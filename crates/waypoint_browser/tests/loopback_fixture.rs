#[path = "support/loopback.rs"]
pub mod loopback;

use loopback::HttpFixture;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

#[test]
fn routed_responses_survive_idle_connections_and_fragmented_redirect_requests() {
    use loopback::Response;
    let fixture = HttpFixture::start_routes(vec![
        (
            "/redirect",
            Response {
                status: 302,
                location: Some("/form".into()),
                body: String::new(),
            },
        ),
        (
            "/form",
            Response {
                status: 200,
                location: None,
                body: "<input name='email'>".into(),
            },
        ),
    ]);
    let _idle = connect(&fixture);
    for (path, status) in [("/redirect", 302), ("/form", 200), ("/missing", 404)] {
        let mut client = connect(&fixture);
        client.write_all(b"GE").unwrap();
        std::thread::sleep(Duration::from_millis(10));
        write!(client, "T {path} HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();
        let mut text = String::new();
        client.read_to_string(&mut text).unwrap();
        assert!(text.starts_with(&format!("HTTP/1.1 {status} ")));
        if status == 302 {
            assert!(text.contains("Location: /form\r\n"));
        }
        if status == 200 {
            assert!(text.ends_with("<input name='email'>"));
        }
    }
    fixture.finish();
}

#[test]
fn observed_channels_wait_for_complete_bodies_and_tolerate_idle_connections() {
    let observed = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let received = observed.clone();
    let fixture = HttpFixture::start_observing(move |path| {
        received.lock().unwrap().push(path.to_owned());
        Ok(())
    });
    let _idle = connect(&fixture);
    let mut client = connect(&fixture);
    client
        .write_all(b"POST /fictional HTTP/1.1\r\nHost: localhost\r\nContent-Length: 7\r\n\r\nfix")
        .unwrap();
    std::thread::sleep(Duration::from_millis(30));
    assert!(observed.lock().unwrap().is_empty());
    client.write_all(b"ture").unwrap();
    let mut response = String::new();
    client.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 204"));
    assert_eq!(observed.lock().unwrap().as_slice(), &["/fictional"]);
    assert_eq!(fixture.posts(), 1);
    fixture.finish();
}

#[test]
fn observed_ipv6_requests_use_loopback_only() {
    let observed = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let received = observed.clone();
    let fixture = HttpFixture::start_observing_on("[::1]:0".parse().unwrap(), move |path| {
        received.lock().unwrap().push(path.to_owned());
        Ok(())
    });
    let mut client = connect(&fixture);
    client
        .write_all(b"GET /ipv6 HTTP/1.1\r\nHost: [::1]\r\n\r\n")
        .unwrap();
    let mut response = String::new();
    client.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 204"));
    assert_eq!(observed.lock().unwrap().as_slice(), &["/ipv6"]);
    fixture.finish();
}

#[test]
fn observed_fixture_refuses_non_loopback_addresses() {
    assert!(std::panic::catch_unwind(|| HttpFixture::start_observing_on(
        "0.0.0.0:0".parse().unwrap(),
        |_| Ok(())
    ))
    .is_err());
    assert!(std::panic::catch_unwind(|| HttpFixture::start_observing_on(
        "[::]:0".parse().unwrap(),
        |_| Ok(())
    ))
    .is_err());
}

fn connect(fixture: &HttpFixture) -> TcpStream {
    let stream = TcpStream::connect(fixture.origin.strip_prefix("http://").unwrap()).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stream
}

#[test]
fn idle_preconnections_do_not_consume_the_page_request() {
    let fixture = HttpFixture::start("fictional page");
    let _idle = connect(&fixture);
    let mut client = connect(&fixture);
    client
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .unwrap();
    let mut response = String::new();
    client.read_to_string(&mut response).unwrap();
    assert!(response.ends_with("fictional page"));
    assert_eq!(fixture.posts(), 0);
    fixture.finish();
}

#[test]
fn fragmented_headers_and_body_are_received_before_counting_a_post() {
    let fixture = HttpFixture::start("");
    let mut client = connect(&fixture);
    for fragment in [
        &b"PO"[..],
        b"ST /fictional HTTP/1.1\r\nHost: local",
        b"host\r\nContent-Length: 7\r\n\r\nfix",
    ] {
        client.write_all(fragment).unwrap();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        fixture.posts(),
        0,
        "partial body was counted as a complete POST"
    );
    client.write_all(b"ture").unwrap();
    let mut response = String::new();
    client.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
    assert_eq!(fixture.posts(), 1);
    fixture.finish();
}

#[test]
fn dropping_during_unwind_closes_listener_and_idle_clients() {
    let fixture = HttpFixture::start("");
    let address = fixture.origin.strip_prefix("http://").unwrap().to_owned();
    let mut idle = connect(&fixture);
    let result = std::thread::spawn(move || {
        let _fixture = fixture;
        panic!("fictional test failure");
    })
    .join();
    assert!(result.is_err());
    assert!(TcpStream::connect(address).is_err());
    match idle.read(&mut [0u8; 1]) {
        Ok(0) => (),
        Err(error) if error.kind() == std::io::ErrorKind::ConnectionReset => (),
        result => panic!("idle fixture connection remained open: {result:?}"),
    }
}

#[test]
fn malformed_or_truncated_requests_surface_as_fixture_failures() {
    for request in [
        &b"POST / HTTP/1.1\r\nContent-Length: 9000\r\n\r\n"[..],
        b"POST / HTTP/1.1\r\nContent-Length: 1\r\nContent-Length: 1\r\n\r\nx",
        b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n",
        b"POST / HTTP/1.1\r\nContent-Length: 7\r\n\r\nshort",
    ] {
        let fixture = HttpFixture::start("");
        let mut client = connect(&fixture);
        client.write_all(request).unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();
        let mut response = vec![];
        let _ = client.read_to_end(&mut response);
        assert_eq!(fixture.posts(), 0);
        assert!(std::thread::spawn(move || fixture.finish()).join().is_err());
    }
}

#[test]
fn observed_page_waits_for_complete_headers_with_an_idle_connection() {
    let observed = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let received = observed.clone();
    let fixture = HttpFixture::start_with_observer("fictional page", move |path| {
        received.lock().unwrap().push(path.to_owned());
        Ok(())
    });
    let _idle = connect(&fixture);
    let mut client = connect(&fixture);
    client.write_all(b"GE").unwrap();
    std::thread::sleep(Duration::from_millis(30));
    assert!(observed.lock().unwrap().is_empty());
    client
        .write_all(b"T /fixture HTTP/1.1\r\nHost: localhost\r\n")
        .unwrap();
    std::thread::sleep(Duration::from_millis(30));
    assert!(observed.lock().unwrap().is_empty());
    client.write_all(b"\r\n").unwrap();
    let mut response = String::new();
    client.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 200 OK"));
    assert!(response.ends_with("fictional page"));
    assert_eq!(observed.lock().unwrap().as_slice(), &["/fixture"]);
    fixture.finish();
}

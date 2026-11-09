#[path = "support/loopback.rs"]
mod loopback;

use loopback::HttpFixture;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

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

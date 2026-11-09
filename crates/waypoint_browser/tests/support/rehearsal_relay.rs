//! Bounded loopback-only TURN request receivers; never a functioning relay.
//! No payload, address or credential contents are retained.
use std::io::{self, Read};
use std::net::{SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug)]
pub enum Transport {
    Udp,
    Tcp,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Counts {
    pub connections: usize,
    pub packets: usize,
    pub allocations: usize,
}

#[derive(Default)]
struct State {
    stop: AtomicBool,
    connections: AtomicUsize,
    packets: AtomicUsize,
    allocations: AtomicUsize,
}

pub fn is_allocate_request(packet: &[u8]) -> bool {
    packet.len() >= 20
        && packet[..2] == [0, 3]
        && packet[4..8] == [0x21, 0x12, 0xa4, 0x42]
        && u16::from_be_bytes([packet[2], packet[3]]) as usize + 20 == packet.len()
        && packet.len().is_multiple_of(4)
}

fn observe(state: &State, packet: &[u8]) -> io::Result<()> {
    if state.packets.fetch_add(1, Ordering::AcqRel) >= 128 {
        return Err(io::Error::other("relay packet budget exceeded"));
    }
    if is_allocate_request(packet) {
        state.allocations.fetch_add(1, Ordering::AcqRel);
    }
    Ok(())
}

enum Listener {
    Udp(UdpSocket),
    Tcp(TcpListener),
}

pub struct RelayFixture {
    pub url: String,
    state: Arc<State>,
    worker: Option<JoinHandle<io::Result<()>>>,
}

fn serve_tcp(mut client: TcpStream, state: &State) -> io::Result<()> {
    client.set_read_timeout(Some(Duration::from_millis(100)))?;
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 2048];
    loop {
        if state.stop.load(Ordering::Acquire) || Instant::now() >= deadline {
            return Ok(());
        }
        match client.read(&mut buffer) {
            Ok(0) => return Ok(()),
            Ok(n) => bytes.extend_from_slice(&buffer[..n]),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                ) =>
            {
                continue
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::ConnectionReset | io::ErrorKind::ConnectionAborted
                ) =>
            {
                return Ok(())
            }
            Err(error) => return Err(error),
        }
        if bytes.len() > 4096 {
            return Err(io::Error::other("relay frame budget exceeded"));
        }
        if bytes.len() >= 20 {
            let size = 20 + u16::from_be_bytes([bytes[2], bytes[3]]) as usize;
            if size > 4096 {
                return Err(io::Error::other("relay frame budget exceeded"));
            }
            if bytes.len() >= size {
                observe(state, &bytes[..size])?;
                return Ok(());
            }
        }
    }
}

fn join(worker: JoinHandle<io::Result<()>>) -> io::Result<()> {
    worker
        .join()
        .map_err(|_| io::Error::other("relay fixture panicked"))?
}

impl RelayFixture {
    pub fn start(address: SocketAddr, transport: Transport) -> Self {
        assert!(
            address.ip().is_loopback() && address.port() == 0,
            "relay fixtures require an ephemeral loopback address"
        );
        let (listener, bound) = match transport {
            Transport::Udp => {
                let socket = UdpSocket::bind(address).unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_millis(100)))
                    .unwrap();
                let bound = socket.local_addr().unwrap();
                (Listener::Udp(socket), bound)
            }
            Transport::Tcp => {
                let socket = TcpListener::bind(address).unwrap();
                socket.set_nonblocking(true).unwrap();
                let bound = socket.local_addr().unwrap();
                (Listener::Tcp(socket), bound)
            }
        };
        let protocol = match transport {
            Transport::Udp => "udp",
            Transport::Tcp => "tcp",
        };
        let url = format!("turn:{bound}?transport={protocol}");
        let state = Arc::new(State::default());
        let received = state.clone();
        let worker = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(90);
            let mut clients: Vec<JoinHandle<io::Result<()>>> = Vec::new();
            let mut work = || -> io::Result<()> {
                let mut buffer = [0u8; 4097];
                while !received.stop.load(Ordering::Acquire) {
                    if Instant::now() >= deadline {
                        return Err(io::Error::other("relay fixture deadline exceeded"));
                    }
                    let mut i = 0;
                    while i < clients.len() {
                        if clients[i].is_finished() {
                            join(clients.swap_remove(i))?;
                        } else {
                            i += 1;
                        }
                    }
                    match &listener {
                        Listener::Udp(socket) => match socket.recv_from(&mut buffer) {
                            Ok((n, source)) => {
                                if !source.ip().is_loopback() || n > 4096 {
                                    return Err(io::Error::other("invalid relay datagram"));
                                }
                                observe(&received, &buffer[..n])?;
                            }
                            Err(error)
                                if matches!(
                                    error.kind(),
                                    io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                                ) => {}
                            Err(error) => return Err(error),
                        },
                        Listener::Tcp(socket) => match socket.accept() {
                            Ok((client, source)) => {
                                if !source.ip().is_loopback()
                                    || clients.len() >= 8
                                    || received.connections.fetch_add(1, Ordering::AcqRel) >= 128
                                {
                                    return Err(io::Error::other(
                                        "relay connection budget exceeded",
                                    ));
                                }
                                let state = received.clone();
                                clients.push(std::thread::spawn(move || serve_tcp(client, &state)));
                            }
                            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                                std::thread::sleep(Duration::from_millis(2))
                            }
                            Err(error) => return Err(error),
                        },
                    }
                }
                Ok(())
            };
            let result = work();
            received.stop.store(true, Ordering::Release);
            let mut failure = result.err();
            for client in clients {
                if let Err(error) = join(client) {
                    failure.get_or_insert(error);
                }
            }
            failure.map_or(Ok(()), Err)
        });
        Self {
            url,
            state,
            worker: Some(worker),
        }
    }

    pub fn counts(&self) -> Counts {
        Counts {
            connections: self.state.connections.load(Ordering::Acquire),
            packets: self.state.packets.load(Ordering::Acquire),
            allocations: self.state.allocations.load(Ordering::Acquire),
        }
    }

    pub fn wait_for_baseline(&self) -> bool {
        let deadline = Instant::now() + Duration::from_secs(8);
        while Instant::now() < deadline {
            if self.counts().allocations > 0 {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }

    pub fn finish(mut self) {
        self.shutdown().unwrap();
    }

    fn shutdown(&mut self) -> io::Result<()> {
        self.state.stop.store(true, Ordering::Release);
        self.worker.take().map_or(Ok(()), join)
    }
}

impl Drop for RelayFixture {
    fn drop(&mut self) {
        if let Err(error) = self.shutdown() {
            eprintln!("Fictional relay cleanup failed: {error}");
        }
    }
}

pub fn page(url: &str) -> String {
    assert!(url.starts_with("turn:127.0.0.1:") || url.starts_with("turn:[::1]:"));
    let encoded = serde_json::to_string(url).unwrap();
    format!(
        r#"<!doctype html><meta charset="utf-8"><label for="name">Fictional name</label><input id="name"><span id="status"></span>
<script>document.querySelector('#name').addEventListener('input', () => {{
  const status=document.querySelector('#status'); if(status.dataset.invoked) return;
  status.dataset.invoked='true';
  try {{
    window.peer=new RTCPeerConnection({{iceTransportPolicy:'relay',iceServers:[{{urls:{encoded},username:'fictional',credential:'fictional'}}]}});
    window.channel=peer.createDataChannel('fictional');
    peer.onicecandidateerror=event=>status.dataset.iceError=String(event.errorCode);
    peer.createOffer().then(offer=>peer.setLocalDescription(offer))
      .then(()=>status.dataset.started='true').catch(error=>status.dataset.error=error.name);
  }} catch(error) {{ status.dataset.error=error.name; }}
}});</script>"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn packet() -> Vec<u8> {
        let mut packet = vec![0, 3, 0, 8, 0x21, 0x12, 0xa4, 0x42];
        packet.extend_from_slice(&[1; 12]);
        packet.extend_from_slice(&[0, 0x19, 0, 4, 17, 0, 0, 0]);
        packet
    }

    #[test]
    fn allocate_framing_rejects_truncation_wrong_magic_and_other_methods() {
        let good = packet();
        assert!(is_allocate_request(&good));
        assert!(!is_allocate_request(&good[..19]));
        assert!(!is_allocate_request(&good[..27]));
        let mut wrong = good.clone();
        wrong[4] = 0;
        assert!(!is_allocate_request(&wrong));
        let mut wrong = good.clone();
        wrong[1] = 1;
        assert!(!is_allocate_request(&wrong));
        let mut wrong = good.clone();
        wrong[3] = 9;
        assert!(!is_allocate_request(&wrong));
    }

    #[test]
    fn udp_and_fragmented_tcp_receivers_observe_loopback_allocations() {
        for address in ["127.0.0.1:0", "[::1]:0"] {
            for transport in [Transport::Udp, Transport::Tcp] {
                let fixture = RelayFixture::start(address.parse().unwrap(), transport);
                let peer: SocketAddr = fixture
                    .url
                    .strip_prefix("turn:")
                    .unwrap()
                    .split('?')
                    .next()
                    .unwrap()
                    .parse()
                    .unwrap();
                match transport {
                    Transport::Udp => {
                        let bind = if peer.is_ipv4() {
                            "127.0.0.1:0"
                        } else {
                            "[::1]:0"
                        };
                        let socket = UdpSocket::bind(bind).unwrap();
                        socket.send_to(&packet(), peer).unwrap();
                    }
                    Transport::Tcp => {
                        let mut socket = TcpStream::connect(peer).unwrap();
                        socket.write_all(&packet()[..12]).unwrap();
                        std::thread::sleep(Duration::from_millis(30));
                        socket.write_all(&packet()[12..]).unwrap();
                    }
                }
                assert!(fixture.wait_for_baseline());
                assert_eq!(fixture.counts().allocations, 1);
                fixture.finish();
            }
        }
    }
}

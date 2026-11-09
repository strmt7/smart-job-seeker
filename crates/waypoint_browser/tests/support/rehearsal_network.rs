//! A bounded receiver for fictional traffic. It records channel names only.
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const CHANNELS: [&str; 9] = [
    "fetch", "beacon", "socket", "events", "image", "frame", "popup", "form", "worker",
];

pub struct Receiver {
    pub origin: String,
    counts: Arc<Mutex<BTreeMap<String, usize>>>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<Result<(), String>>>,
}

impl Receiver {
    pub fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let counts = Arc::new(Mutex::new(BTreeMap::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (received, stopping) = (counts.clone(), stop.clone());
        let worker = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(90);
            let mut connections = 0;
            while !stopping.load(Ordering::Acquire) {
                if Instant::now() >= deadline {
                    return Err("receiver deadline exceeded".into());
                }
                let (mut client, _) = match listener.accept() {
                    Ok(value) => value,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    Err(e) => return Err(e.to_string()),
                };
                connections += 1;
                if connections > 128 {
                    return Err("receiver connection budget exceeded".into());
                }
                client
                    .set_read_timeout(Some(Duration::from_millis(100)))
                    .map_err(|e| e.to_string())?;
                client
                    .set_write_timeout(Some(Duration::from_millis(100)))
                    .map_err(|e| e.to_string())?;
                let mut header = Vec::new();
                let mut chunk = [0u8; 1024];
                let request_deadline = Instant::now() + Duration::from_secs(1);
                while header.len() < 8192 && Instant::now() < request_deadline {
                    match client.read(&mut chunk) {
                        Ok(0) => break,
                        Ok(n) => header.extend_from_slice(&chunk[..n]),
                        Err(e)
                            if matches!(
                                e.kind(),
                                std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                            ) =>
                        {
                            break
                        }
                        Err(_) => break,
                    }
                    if header.windows(4).any(|v| v == b"\r\n\r\n") {
                        break;
                    }
                }
                if header.is_empty() {
                    continue;
                } // A speculative connection is not a request.
                if !header.windows(4).any(|v| v == b"\r\n\r\n") {
                    return Err("incomplete receiver headers".into());
                }
                let text = std::str::from_utf8(&header).map_err(|_| "invalid fixture request")?;
                let mut first = text
                    .lines()
                    .next()
                    .ok_or("missing request line")?
                    .split_whitespace();
                let _method = first.next().ok_or("missing method")?;
                let path = first
                    .next()
                    .ok_or("missing channel")?
                    .trim_start_matches('/');
                if !CHANNELS.contains(&path) {
                    return Err("unexpected receiver channel".into());
                }
                *received.lock().unwrap().entry(path.to_owned()).or_insert(0) += 1;
                // We measure request arrival, not success of a WebSocket/SSE
                // session. Returning 204 prevents reconnect loops and scripting.
                let _ = client.write_all(b"HTTP/1.1 204 No Content\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Private-Network: true\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            }
            Ok(())
        });
        Self {
            origin,
            counts,
            stop,
            worker: Some(worker),
        }
    }

    pub fn counts(&self) -> BTreeMap<String, usize> {
        self.counts.lock().unwrap().clone()
    }

    pub fn wait_for_baseline(&self) -> bool {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let counts = self.counts();
            if CHANNELS
                .iter()
                .all(|channel| counts.get(*channel).is_some_and(|n| *n > 0))
            {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    pub fn finish(mut self) {
        self.shutdown().unwrap();
    }

    fn shutdown(&mut self) -> Result<(), String> {
        self.stop.store(true, Ordering::Release);
        self.worker.take().map_or(Ok(()), |thread| {
            thread.join().map_err(|_| "receiver panicked")?
        })
    }
}

impl Drop for Receiver {
    fn drop(&mut self) {
        if let Err(error) = self.shutdown() {
            eprintln!("Fictional receiver cleanup: {error}");
        }
    }
}

pub fn page(origin: &str) -> String {
    assert!(origin.starts_with("http://127.0.0.1:"));
    format!(
        r#"<!doctype html><meta charset="utf-8">
<label for="name">Fictional name</label><input id="name"><span id="status" data-fired="false"></span>
<iframe name="destination" hidden></iframe><form id="fictional" target="destination" method="post" action="{origin}/form"><input name="name" value="Fictional"></form>
<script>
document.querySelector('#name').addEventListener('input', () => {{
  const status = document.querySelector('#status');
  if (status.dataset.fired === 'true') return;
  status.dataset.fired = 'true';
  fetch('{origin}/fetch', {{method:'POST',mode:'no-cors',body:'fictional'}}).catch(()=>{{}});
  navigator.sendBeacon('{origin}/beacon','fictional');
  window.socket = new WebSocket('{}');
  window.events = new EventSource('{origin}/events');
  const image = new Image(); image.src='{origin}/image'; document.body.append(image);
  const frame = document.createElement('iframe'); frame.src='{origin}/frame'; document.body.append(frame);
  window.open('{origin}/popup','_blank');
  document.querySelector('#fictional').submit();
  try {{
    window.worker = new Worker(URL.createObjectURL(new Blob(["postMessage('started');fetch('{origin}/worker',{{mode:'no-cors'}}).then(()=>postMessage('completed')).catch(error=>postMessage(error.name))"],{{type:'application/javascript'}})));
    window.worker.onmessage = event => status.dataset.worker=event.data;
    window.worker.onerror = event => status.dataset.workerError=event.message;
  }} catch(error) {{ status.dataset.workerError=error.name; }}
}});
</script>"#,
        origin.replace("http:", "ws:") + "/socket"
    )
}

//! Channel counting on the shared bounded, concurrent fictional HTTP server.
use crate::loopback::HttpFixture;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const CHANNELS: [&str; 9] = [
    "fetch", "beacon", "socket", "events", "image", "frame", "popup", "form", "worker",
];

pub struct Receiver {
    pub origin: String,
    counts: Arc<Mutex<BTreeMap<String, usize>>>,
    fixture: HttpFixture,
}

impl Receiver {
    pub fn start_on(address: std::net::SocketAddr) -> Self {
        let counts = Arc::new(Mutex::new(BTreeMap::new()));
        let received = counts.clone();
        let fixture = HttpFixture::start_observing_on(address, move |path| {
            let channel = path
                .strip_prefix('/')
                .ok_or_else(|| std::io::Error::other("invalid receiver path"))?;
            if !CHANNELS.contains(&channel) {
                return Err(std::io::Error::other("unexpected receiver channel"));
            }
            *received
                .lock()
                .unwrap()
                .entry(channel.to_owned())
                .or_insert(0) += 1;
            Ok(())
        });
        Self {
            origin: fixture.origin.clone(),
            counts,
            fixture,
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
    pub fn finish(self) {
        self.fixture.finish();
    }
}

pub fn page(origin: &str) -> String {
    let address: std::net::SocketAddr = origin.strip_prefix("http://").unwrap().parse().unwrap();
    assert!(address.ip().is_loopback() && address.port() != 0);
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
  status.dataset.completed='true';
}});
</script>"#,
        origin.replace("http:", "ws:") + "/socket"
    )
}

//! Windows notification-area controller; no application window is created.
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};
use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem},
    Icon, TrayIcon, TrayIconBuilder,
};
use winit::{
    event::{Event, StartCause},
    event_loop::{ControlFlow, EventLoop},
};

pub fn run(url: String, stop: Arc<AtomicBool>, worker: JoinHandle<()>) -> Result<(), String> {
    let event_loop = EventLoop::new().map_err(|e| e.to_string())?;
    let menu = Menu::new();
    let open = MenuItem::new("Open Waypoint", true, None);
    let quit = MenuItem::new("Exit", true, None);
    menu.append(&open).map_err(|e| e.to_string())?;
    menu.append(&quit).map_err(|e| e.to_string())?;
    let mut icon: Option<TrayIcon> = None;
    let mut shutting_down = false;
    #[allow(deprecated)]
    event_loop
        .run(move |event, event_loop| {
            event_loop.set_control_flow(ControlFlow::WaitUntil(
                Instant::now() + Duration::from_millis(250),
            ));
            if matches!(event, Event::NewEvents(StartCause::Init)) {
                let mut pixels = vec![0u8; 32 * 32 * 4];
                for y in 0..32 {
                    for x in 0..32 {
                        let i = (y * 32 + x) * 4;
                        let mark =
                            (7..25).contains(&x) && (8..24).contains(&y) && ((x + y) % 10 < 3);
                        pixels[i..i + 4].copy_from_slice(if mark {
                            &[255, 255, 255, 255]
                        } else {
                            &[25, 115, 238, 255]
                        });
                    }
                }
                if let Ok(image) = Icon::from_rgba(pixels, 32, 32) {
                    icon = TrayIconBuilder::new()
                        .with_tooltip("Waypoint — local job workspace")
                        .with_icon(image)
                        .with_menu(Box::new(menu.clone()))
                        .build()
                        .ok();
                }
                if icon.is_none() {
                    stop.store(true, Ordering::Release);
                    shutting_down = true;
                }
            }
            while let Ok(event) = MenuEvent::receiver().try_recv() {
                if event.id == *open.id() && !shutting_down {
                    let _ = webbrowser::open(&url);
                }
                if event.id == *quit.id() {
                    stop.store(true, Ordering::Release);
                    shutting_down = true;
                    let _ = quit.set_text("Closing server…");
                }
            }
            if worker.is_finished() {
                event_loop.exit();
            }
            if shutting_down && worker.is_finished() {
                event_loop.exit();
            }
        })
        .map_err(|e| e.to_string())
}

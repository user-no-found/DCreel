use crate::desktop_notifications;
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Manager, WebviewWindow};
use windows::Win32::{
    Foundation::{HWND, RECT},
    UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowRect, IsWindowVisible},
};

pub(super) fn notification_id(app: &AppHandle) -> String {
    desktop_notifications::current_desktop_notification(app.state())
        .unwrap()
        .unwrap()
        .id
}

pub(super) fn visible(window: &WebviewWindow) -> bool {
    let hwnd = HWND(window.hwnd().unwrap().0);
    unsafe { IsWindowVisible(hwnd).as_bool() }
}

pub(super) fn wait_for(description: &str, timeout: Duration, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + timeout;
    while !condition() {
        assert!(Instant::now() < deadline, "timed out: {description}");
        thread::sleep(Duration::from_millis(30));
    }
}

pub(super) fn javascript(window: &WebviewWindow, script: &str) -> serde_json::Value {
    let (sender, receiver) = mpsc::channel();
    window
        .eval_with_callback(script, move |result| {
            let _ = sender.send(result);
        })
        .unwrap();
    let result = receiver.recv_timeout(Duration::from_secs(5)).unwrap();
    serde_json::from_str(&result).unwrap_or(serde_json::Value::Null)
}

pub(super) fn on_ui(app: &AppHandle, action: impl FnOnce() + Send + 'static) {
    let (sender, receiver) = mpsc::channel();
    app.run_on_main_thread(move || {
        let result = catch_unwind(AssertUnwindSafe(action));
        let _ = sender.send(result);
    })
    .unwrap();
    if let Err(error) = receiver.recv_timeout(Duration::from_secs(5)).unwrap() {
        std::panic::resume_unwind(error);
    }
}

pub(super) fn assert_separate(a: &WebviewWindow, b: &WebviewWindow) {
    let rect = |window: &WebviewWindow| {
        let mut rect = RECT::default();
        unsafe { GetWindowRect(HWND(window.hwnd().unwrap().0), &mut rect) }.unwrap();
        rect
    };
    let (a, b) = (rect(a), rect(b));
    assert!(
        a.right <= b.left || b.right <= a.left || a.bottom <= b.top || b.bottom <= a.top,
        "popup HWND rectangles overlap: {a:?}, {b:?}"
    );
}

pub(super) fn show(app: &AppHandle) {
    let ui_app = app.clone();
    on_ui(app, move || {
        let foreground = unsafe { GetForegroundWindow() };
        desktop_notifications::show_transfer_progress(&ui_app);
        assert_eq!(
            unsafe { GetForegroundWindow() },
            foreground,
            "show stole foreground focus"
        );
    });
}

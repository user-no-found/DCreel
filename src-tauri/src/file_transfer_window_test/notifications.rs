use super::support::*;
use crate::desktop_notifications;
use std::{
    thread,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Manager, WebviewWindow};

pub(super) fn exercise(app: &AppHandle, window: &WebviewWindow) {
    let notification = app.get_webview_window("desktop-notification").unwrap();
    desktop_notifications::show_message(app, "窗口测试", "不会移动文件");
    wait_for("notification close button", Duration::from_secs(5), || {
        javascript(
            &notification,
            "!!document.querySelector('.desktop-notification-close')",
        ) == true
    });
    wait_for(
        "render acknowledgement shows notification",
        Duration::from_secs(3),
        || visible(&notification),
    );
    assert!(visible(&notification));
    assert_separate(&window, &notification);
    javascript(
        &notification,
        "document.querySelector('.desktop-notification-close').click()",
    );
    wait_for(
        "notification close hides HWND",
        Duration::from_secs(2),
        || !visible(&notification),
    );
    println!("PASS: notification close button -> IPC -> native hide");

    let ui_window = window.clone();
    on_ui(app, move || {
        crate::dismiss_auxiliary_window(ui_window, None).unwrap()
    });
    desktop_notifications::show_message(app, "通知先出现", "测试反向排列");
    wait_for("notification first", Duration::from_secs(3), || {
        visible(&notification)
    });
    let old_id = notification_id(app);
    show(app);
    assert_separate(&window, &notification);
    println!("PASS: both popup creation orders use non-overlapping HWND rectangles");

    desktop_notifications::show_update(app, "window-test".into());
    wait_for(
        "replacement update rendered",
        Duration::from_secs(3),
        || {
            visible(&notification)
                && javascript(
                    &notification,
                    "document.body.textContent.includes('window-test')",
                ) == true
        },
    );
    let ui_window = notification.clone();
    on_ui(app, move || {
        crate::dismiss_auxiliary_window(ui_window, Some(old_id)).unwrap()
    });
    assert!(
        visible(&notification),
        "stale close hid replacement notification"
    );
    thread::sleep(Duration::from_millis(9_200));
    assert!(
        visible(&notification),
        "old notification timeout hid persistent update"
    );
    notification.reload().unwrap();
    wait_for(
        "notification restored after page reload",
        Duration::from_secs(5),
        || {
            visible(&notification)
                && javascript(
                    &notification,
                    "document.body.textContent.includes('window-test')",
                ) == true
        },
    );
    notification.close().unwrap();
    wait_for("native notification close", Duration::from_secs(2), || {
        !visible(&notification)
    });
    assert!(app.get_webview_window("desktop-notification").is_some());
    println!(
        "PASS: stale manual/timeout close ignored; update persists; reload and native close work"
    );

    desktop_notifications::show_message(app, "自动收起测试", "由后端计时，不依赖前端定时器");
    wait_for("timed notification shown", Duration::from_secs(3), || {
        visible(&notification)
    });
    let started = Instant::now();
    thread::sleep(Duration::from_secs(8));
    assert!(
        visible(&notification),
        "ordinary notification closed too early"
    );
    wait_for(
        "native 9-second notification dismissal",
        Duration::from_secs(3),
        || !visible(&notification),
    );
    println!(
        "PASS: notification auto-dismissed HWND after {:?}",
        started.elapsed()
    );
}

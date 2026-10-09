use super::support::{javascript, on_ui, visible, wait_for};
use std::{sync::mpsc, thread, time::Duration};
use tauri::{AppHandle, Manager};

pub(super) fn exercise(app: &AppHandle) {
    let main = app.get_webview_window("main").unwrap();
    main.reload().unwrap();
    wait_for("main settings UI ready", Duration::from_secs(10), || {
        javascript(&main, "!!document.querySelector('button[title=设置]')") == true
    });
    javascript(
        &main,
        "document.querySelector('button[title=设置]').click()",
    );
    wait_for("hotkey input rendered", Duration::from_secs(3), || {
        javascript(&main, "!!document.querySelector('.hotkey-row input')") == true
    });

    let (held, acquired) = mpsc::channel();
    let (release, wait_release) = mpsc::channel();
    let lock_app = app.clone();
    let lock_worker = thread::spawn(move || {
        lock_app
            .state::<crate::desktop_host::DesktopHostController>()
            .with_control_locked_for_test(|| {
                held.send(()).unwrap();
                wait_release.recv_timeout(Duration::from_secs(15)).unwrap();
            });
    });
    acquired.recv_timeout(Duration::from_secs(3)).unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        assert!(
            javascript(
                &main,
                r#"(() => {
            const input = document.querySelector('.hotkey-row input');
            input.focus();
            window.__captureProbe = { done: false };
            window.__TAURI_INTERNALS__.invoke('set_hotkey_capture_active', {active: true})
              .then(() => window.__captureProbe.done = true);
            return true;
        })()"#
            ) == true
        );
        thread::sleep(Duration::from_millis(150));
        assert!(javascript(&main, "window.__captureProbe.done === false") == true);
        assert!(
            javascript(
                &main,
                r#"(() => {
            const input = document.querySelector('.hotkey-row input');
            const key = (type, key, code, ctrlKey) => input.dispatchEvent(
              new KeyboardEvent(type, {key, code, ctrlKey, bubbles: true, cancelable: true}));
            key('keydown', 'Control', 'ControlLeft', true);
            const prevented = !key('keydown', 's', 'KeyS', true);
            key('keyup', 's', 'KeyS', true);
            key('keyup', 'Control', 'ControlLeft', false);
            return prevented;
        })()"#
            ) == true
        );
        wait_for("Ctrl+S draft rendered", Duration::from_secs(3), || {
            javascript(
                &main,
                "document.querySelector('.hotkey-row input').value === 'Ctrl+S'",
            ) == true
        });
        wait_for(
            "Ctrl+S saved while capture IO waits",
            Duration::from_secs(3),
            || {
                app.state::<super::FixtureState>()
                    .preferences
                    .lock()
                    .unwrap()
                    .ghost_hotkey
                    == "Ctrl+S"
            },
        );
        assert!(
            javascript(
                &main,
                r#"(() => {
            const input = document.querySelector('.hotkey-row input');
            input.blur(); input.focus(); input.blur();
            document.querySelector('button[title=桌面盒子]').click();
            return true;
        })()"#
            ) == true
        );
        wait_for(
            "navigation responds during capture IO",
            Duration::from_secs(3),
            || {
                javascript(
                    &main,
                    "document.body.textContent.includes('桌面盒子') && !document.querySelector('.hotkey-row input')",
                ) == true
            },
        );
    }));
    release.send(()).unwrap();
    lock_worker.join().unwrap();
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
    wait_for("pending capture completes", Duration::from_secs(3), || {
        javascript(&main, "window.__captureProbe.done === true") == true
    });
    let ui_window = main.clone();
    on_ui(app, move || ui_window.close().unwrap());
    wait_for(
        "main native close remains available",
        Duration::from_secs(3),
        || !visible(&main),
    );
    println!(
        "PASS: Ctrl+S, focus/blur, navigation and native close remain responsive during contended real capture IPC"
    );
}

use super::support::*;
use crate::desktop_notifications;
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tauri::{AppHandle, Manager};
use windows::{
    Win32::{
        System::{
            Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, IPersistFile},
            Ole::{OleInitialize, OleUninitialize},
        },
        UI::Shell::{IShellLinkW, ShellLink},
    },
    core::{Interface, PCWSTR},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "dcreel-notification-shortcut-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn link(&self, target: &Path, name: &str) -> PathBuf {
        let path = self.0.join(name);
        unsafe {
            OleInitialize(None).unwrap();
            {
                let link: IShellLinkW =
                    CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).unwrap();
                link.SetPath(PCWSTR(creel_shell_operations::shell_path(target).as_ptr()))
                    .unwrap();
                let persisted: IPersistFile = link.cast().unwrap();
                persisted
                    .Save(
                        PCWSTR(creel_shell_operations::shell_path(&path).as_ptr()),
                        true,
                    )
                    .unwrap();
            }
            OleUninitialize();
        }
        std::fs::canonicalize(path).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

pub(super) fn exercise(app: &AppHandle) {
    let fixture = Fixture::new();
    let target = fixture.0.join("missing.exe");
    let link = fixture.link(&target, "broken.lnk");
    let window = app.get_webview_window("desktop-notification").unwrap();
    let show = |path: &Path| {
        desktop_notifications::show_broken_shortcut(
            app,
            path.into(),
            format!(
                "无法打开 {}：快捷方式指向的文件已不存在或暂时无法访问：{}。可以删除失效快捷方式。",
                path.display(),
                target.display()
            ),
        );
        wait_for("shortcut action rendered", Duration::from_secs(5), || {
            visible(&window)
                && javascript(
                    &window,
                    "document.body.textContent.includes('删除快捷方式')",
                ) == true
        });
    };
    show(&link);
    assert_eq!(
        javascript(
            &window,
            r#"(() => {
        const card = document.querySelector('.desktop-notification').getBoundingClientRect();
        const inside = (selector) => {
            const rect = document.querySelector(selector).getBoundingClientRect();
            return rect.top >= card.top && rect.bottom <= card.bottom;
        };
        const message = document.querySelector('.desktop-notification-message');
        return inside('.desktop-notification-actions button') && inside('.desktop-notification-close')
            && message.clientHeight > 0 && message.scrollHeight > message.clientHeight;
    })()"#
        ),
        true,
        "long path must scroll while delete and close remain visible"
    );
    let stale_id = notification_id(app);
    javascript(
        &window,
        "document.querySelector('.desktop-notification-close').click()",
    );
    wait_for(
        "action notification X closes",
        Duration::from_secs(2),
        || !visible(&window),
    );
    assert!(link.exists());
    show(&link);
    window.reload().unwrap();
    wait_for(
        "shortcut action restored after reload",
        Duration::from_secs(5),
        || {
            visible(&window)
                && javascript(
                    &window,
                    "document.body.textContent.includes('删除快捷方式')",
                ) == true
        },
    );
    // A command from an already closed notification cannot delete this link.
    assert!(
        tauri::async_runtime::block_on(desktop_notifications::delete_notification_shortcut(
            window.clone(),
            stale_id
        ))
        .is_err()
    );
    assert!(link.exists());
    javascript(
        &window,
        "document.querySelector('.desktop-notification-actions button').click()",
    );
    wait_for(
        "real shortcut recycled and popup dismissed",
        Duration::from_secs(5),
        || !link.exists() && !visible(&window),
    );
    assert!(!target.exists());
    println!(
        "PASS: broken-link delete button -> guarded IPC -> recycle link only; X/reload/stale ID preserved"
    );

    let restored = fixture.link(&target, "restored.lnk");
    show(&restored);
    std::fs::write(&target, b"target recovered").unwrap();
    javascript(
        &window,
        "document.querySelector('.desktop-notification-actions button').click()",
    );
    wait_for(
        "restored target deletion refused with inline reason",
        Duration::from_secs(3),
        || javascript(&window, "document.body.textContent.includes('目标已恢复')") == true,
    );
    assert!(restored.exists());
    assert_eq!(std::fs::read(&target).unwrap(), b"target recovered");
    assert!(visible(&window));
    assert_eq!(
        javascript(
            &window,
            "document.querySelector('.desktop-notification-close').disabled"
        ),
        false
    );
    window.close().unwrap();
    wait_for(
        "native close after delete failure",
        Duration::from_secs(2),
        || !visible(&window),
    );
    assert!(app.get_webview_window("desktop-notification").is_some());
    println!("PASS: restored target is preserved; failed action leaves native close working");
}

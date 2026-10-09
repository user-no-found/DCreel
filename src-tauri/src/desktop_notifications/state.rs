use serde::Serialize;
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};
use tauri::ipc::Channel;

pub(super) const NOTIFICATION_WINDOW: &str = "desktop-notification";
pub(super) const TRANSFER_WINDOW: &str = "transfer-progress";
pub(super) const NOTIFICATION_DURATION: Duration = Duration::from_secs(9);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopNotification {
    pub id: String,
    pub kind: &'static str,
    pub title: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<&'static str>,
    #[serde(skip)]
    pub(super) shortcut_path: Option<std::path::PathBuf>,
}

#[derive(Default)]
pub struct NotificationManager(pub(super) Mutex<NotificationState>);

#[derive(Default)]
pub(super) struct NotificationState {
    pub(super) current: Option<DesktopNotification>,
    pub(super) shown_at: Option<Instant>,
    pub(super) subscriber: Option<Channel<DesktopNotification>>,
    pub(super) deleting_id: Option<String>,
}

impl NotificationState {
    pub(super) fn matches(&self, id: &str) -> bool {
        self.current
            .as_ref()
            .is_some_and(|current| current.id == id)
    }

    pub(super) fn can_expire(&self, id: &str, shown_at: Instant) -> bool {
        self.matches(id)
            && self.shown_at == Some(shown_at)
            && self
                .current
                .as_ref()
                .is_some_and(|current| current.kind == "message")
    }
}

impl NotificationState {
    pub(super) fn complete_shortcut(&mut self, id: &str) -> bool {
        if !self.matches(id) || self.deleting_id.as_deref() != Some(id) {
            return false;
        }
        self.deleting_id = None;
        true
    }

    pub(super) fn claim_shortcut(&mut self, id: &str) -> Result<std::path::PathBuf, String> {
        if !self.matches(id) || self.shown_at.is_none() {
            return Err("通知已关闭或已被替换".into());
        }
        if self.deleting_id.as_deref() == Some(id) {
            return Err("正在删除快捷方式".into());
        }
        let path = self
            .current
            .as_ref()
            .and_then(|current| current.shortcut_path.clone())
            .ok_or("此通知没有可删除的失效快捷方式")?;
        self.deleting_id = Some(id.to_string());
        Ok(path)
    }
}

#[cfg(test)]
mod tests;

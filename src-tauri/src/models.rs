use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub use creel_ipc::{DisplayAnchor, FencePlacement, GhostModeTrigger};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Preferences {
    #[serde(default = "default_title_opacity")]
    pub title_opacity: f64,
    #[serde(default = "default_content_opacity")]
    pub content_opacity: f64,
    #[serde(default = "default_true")]
    pub show_fence_border: bool,
    #[serde(default = "default_fence_border_opacity")]
    pub fence_border_opacity: f64,
    #[serde(default = "default_icon_size")]
    pub icon_size: u32,
    #[serde(default = "default_fence_width")]
    pub default_fence_width: f64,
    #[serde(default = "default_fence_height")]
    pub default_fence_height: f64,
    #[serde(default)]
    pub ghost_mode: bool,
    #[serde(default)]
    pub ghost_mode_trigger: GhostModeTrigger,
    #[serde(default = "default_ghost_opacity")]
    pub ghost_opacity: f64,
    #[serde(default = "default_ghost_hotkey")]
    pub ghost_hotkey: String,
    #[serde(default)]
    pub start_on_boot: bool,
    #[serde(default)]
    pub show_hidden_files: bool,
    #[serde(default = "default_true")]
    pub show_fence_titles: bool,
    #[serde(default = "default_true")]
    pub show_tray_icon: bool,
    #[serde(default = "default_true")]
    pub desktop_mode: bool,
    #[serde(default = "default_true")]
    pub desktop_context_menu: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            title_opacity: default_title_opacity(),
            content_opacity: default_content_opacity(),
            show_fence_border: true,
            fence_border_opacity: default_fence_border_opacity(),
            icon_size: default_icon_size(),
            default_fence_width: default_fence_width(),
            default_fence_height: default_fence_height(),
            ghost_mode: false,
            ghost_mode_trigger: GhostModeTrigger::Automatic,
            ghost_opacity: default_ghost_opacity(),
            ghost_hotkey: default_ghost_hotkey(),
            start_on_boot: false,
            show_hidden_files: false,
            show_fence_titles: true,
            show_tray_icon: true,
            desktop_mode: true,
            desktop_context_menu: true,
        }
    }
}

fn default_title_opacity() -> f64 {
    0.9
}

fn default_content_opacity() -> f64 {
    0.9
}

fn default_fence_border_opacity() -> f64 {
    0.4
}

fn default_icon_size() -> u32 {
    46
}

fn default_fence_width() -> f64 {
    330.0
}

fn default_fence_height() -> f64 {
    280.0
}

fn default_ghost_opacity() -> f64 {
    0.2
}

fn default_ghost_hotkey() -> String {
    "Ctrl+Alt+G".into()
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FenceConfig {
    pub id: String,
    pub title: String,
    pub directory: PathBuf,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub color: String,
    #[serde(default = "default_content_color")]
    pub content_color: String,
    #[serde(default)]
    pub collapsed: bool,
    #[serde(default)]
    pub locked: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_anchor: Option<DisplayAnchor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placement: Option<FencePlacement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedState {
    #[serde(default = "state_version")]
    pub version: u32,
    #[serde(default)]
    pub fences: Vec<FenceConfig>,
    #[serde(default)]
    pub preferences: Preferences,
}

impl Default for PersistedState {
    fn default() -> Self {
        Self {
            version: state_version(),
            fences: Vec::new(),
            preferences: Preferences::default(),
        }
    }
}

pub(crate) fn state_version() -> u32 {
    6
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FenceView {
    #[serde(flatten)]
    pub config: FenceConfig,
    pub item_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Dashboard {
    pub fences: Vec<FenceView>,
    pub preferences: Preferences,
    pub desktop_visible: bool,
}

/// 只允许控制界面修改非几何属性。盒子坐标、尺寸和显示器锚点只能由
/// Desktop Host 的几何事件更新，防止陈旧的控制台快照覆盖刚完成的拖动。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FencePatch {
    pub title: Option<String>,
    pub color: Option<String>,
    pub content_color: Option<String>,
    pub collapsed: Option<bool>,
    pub locked: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreferencesPatch {
    pub title_opacity: Option<f64>,
    pub content_opacity: Option<f64>,
    pub show_fence_border: Option<bool>,
    pub fence_border_opacity: Option<f64>,
    pub icon_size: Option<u32>,
    pub default_fence_width: Option<f64>,
    pub default_fence_height: Option<f64>,
    pub ghost_mode: Option<bool>,
    pub ghost_mode_trigger: Option<GhostModeTrigger>,
    pub ghost_opacity: Option<f64>,
    pub ghost_hotkey: Option<String>,
    pub start_on_boot: Option<bool>,
    pub show_hidden_files: Option<bool>,
    pub show_fence_titles: Option<bool>,
    pub show_tray_icon: Option<bool>,
    pub desktop_mode: Option<bool>,
    pub desktop_context_menu: Option<bool>,
}

impl PreferencesPatch {
    pub fn apply_to(self, preferences: &mut Preferences) {
        macro_rules! apply {
            ($field:ident) => {
                if let Some(value) = self.$field {
                    preferences.$field = value;
                }
            };
        }

        apply!(title_opacity);
        apply!(content_opacity);
        apply!(show_fence_border);
        apply!(fence_border_opacity);
        apply!(icon_size);
        apply!(default_fence_width);
        apply!(default_fence_height);
        apply!(ghost_mode);
        apply!(ghost_mode_trigger);
        apply!(ghost_opacity);
        apply!(ghost_hotkey);
        apply!(start_on_boot);
        apply!(show_hidden_files);
        apply!(show_fence_titles);
        apply!(show_tray_icon);
        apply!(desktop_mode);
        apply!(desktop_context_menu);
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewFenceInput {
    pub title: String,
    pub color: String,
    #[serde(default = "default_content_color")]
    pub content_color: String,
    pub directory: Option<PathBuf>,
}

fn default_content_color() -> String {
    "paper".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_preferences_enable_desktop_mode() {
        let preferences: Preferences = serde_json::from_str("{}").unwrap();
        assert!(preferences.desktop_mode);
        assert_eq!(preferences.icon_size, 46);
        assert_eq!(preferences.title_opacity, 0.9);
        assert_eq!(preferences.content_opacity, 0.9);
        assert!(preferences.show_fence_border);
        assert_eq!(preferences.fence_border_opacity, 0.4);
        assert_eq!(preferences.default_fence_width, 330.0);
        assert_eq!(preferences.default_fence_height, 280.0);
        assert!(preferences.show_fence_titles);
        assert!(preferences.show_tray_icon);
        assert_eq!(preferences.ghost_mode_trigger, GhostModeTrigger::Automatic);
        assert_eq!(preferences.ghost_opacity, 0.2);
        assert_eq!(preferences.ghost_hotkey, "Ctrl+Alt+G");
    }

    #[test]
    fn preference_patch_changes_only_present_fields() {
        let mut preferences = Preferences::default();
        PreferencesPatch {
            icon_size: Some(60),
            show_tray_icon: Some(false),
            ..PreferencesPatch::default()
        }
        .apply_to(&mut preferences);

        assert_eq!(preferences.icon_size, 60);
        assert!(!preferences.show_tray_icon);
        assert_eq!(preferences.title_opacity, 0.9);
        assert!(preferences.desktop_context_menu);
    }
}

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const PROTOCOL_VERSION: u16 = 15;
pub const ARG_SHOW: &str = "--show";
pub const ARG_SILENT: &str = "--silent";
pub const ARG_NEW_FENCE: &str = "--new-fence";
pub const ARG_NEW_MAPPED_FENCE: &str = "--new-mapped-fence";
pub const ARG_TOGGLE_FENCES: &str = "--toggle-fences";
pub const ARG_QUIT: &str = "--quit";
pub const ARG_MAP_FOLDER: &str = "--map-folder";
pub const EXPLORER_COMMAND_CLSID_TEXT: &str = "{7C998A5B-2F68-4A76-9C88-7209A70F4CA0}";
pub const EXPLORER_COMMAND_CANONICAL_GUID_TEXT: &str = "{06E8DE01-87AE-4DB7-A685-E8E87F5F8F8B}";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "command", content = "payload", rename_all = "snake_case")]
pub enum ExternalCommand {
    Show,
    Silent,
    NewStorageBox,
    NewMappedBox,
    ToggleFences,
    Quit,
    MapFolder(PathBuf),
}

pub fn parse_external_commands(args: &[String]) -> Vec<ExternalCommand> {
    let mut commands = Vec::new();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            ARG_SHOW => commands.push(ExternalCommand::Show),
            ARG_SILENT => commands.push(ExternalCommand::Silent),
            ARG_NEW_FENCE => commands.push(ExternalCommand::NewStorageBox),
            ARG_NEW_MAPPED_FENCE => commands.push(ExternalCommand::NewMappedBox),
            ARG_TOGGLE_FENCES => commands.push(ExternalCommand::ToggleFences),
            ARG_QUIT => commands.push(ExternalCommand::Quit),
            ARG_MAP_FOLDER => {
                if let Some(path) = args.get(index + 1) {
                    commands.push(ExternalCommand::MapFolder(PathBuf::from(path)));
                    index += 1;
                }
            }
            _ => {}
        }
        index += 1;
    }
    commands
}

pub fn map_folder_args(path: PathBuf) -> [std::ffi::OsString; 2] {
    [ARG_MAP_FOLDER.into(), path.into_os_string()]
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayAnchor {
    pub device_name: String,
    pub work_left: i32,
    pub work_top: i32,
    pub work_width: i32,
    pub work_height: i32,
    /// 显示器的原始物理 DPI。保留现有字段名以供物理距离换算。
    pub dpi_x: u32,
    pub dpi_y: u32,
    /// Windows 当前为该显示器选择的有效 DPI（例如 125% 为 120）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_dpi_x: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_dpi_y: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayoutAnchor {
    Start,
    End,
    Proportional,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LayoutAxis {
    pub anchor: LayoutAnchor,
    /// Start/End 时为 96-DPI DIP 边距，Proportional 时为 0..=1 的可移动区比例。
    pub value: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FencePlacement {
    /// 同一相连盒子组共享的稳定标识；组发生拆分或合并时会重新计算。
    pub group_id: String,
    pub horizontal: LayoutAxis,
    pub vertical: LayoutAxis,
    /// 相对于盒子组左上角的 96-DPI DIP 偏移。
    pub offset_x_dip: f64,
    pub offset_y_dip: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostFenceSnapshot {
    pub id: String,
    pub title: String,
    pub directory: PathBuf,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub color: String,
    pub content_color: String,
    pub collapsed: bool,
    pub locked: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_anchor: Option<DisplayAnchor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placement: Option<FencePlacement>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GhostModeTrigger {
    #[default]
    Automatic,
    Hotkey,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostPreferencesSnapshot {
    pub title_opacity: f64,
    pub content_opacity: f64,
    pub show_fence_border: bool,
    pub fence_border_opacity: f64,
    pub icon_size: u32,
    pub ghost_mode: bool,
    pub ghost_mode_trigger: GhostModeTrigger,
    pub ghost_opacity: f64,
    pub ghost_hotkey: String,
    pub show_hidden_files: bool,
    pub show_fence_titles: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum HostCommand {
    Hello {
        protocol_version: u16,
    },
    Sync {
        revision: u64,
        fences: Vec<HostFenceSnapshot>,
        preferences: HostPreferencesSnapshot,
        visible: bool,
    },
    RefreshFence {
        id: String,
    },
    SetHotkeyCapture {
        active: bool,
    },
    Shutdown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum HostUserAction {
    CreateStorageBox,
    CreateMappedBox,
    RenameFence { id: String, title: String },
    ToggleFenceCollapsed { id: String },
    ToggleFenceLocked { id: String },
    SetFenceColor { id: String, color: String },
    ResetFenceSize { id: String },
    RemoveFence { id: String },
    QuitApplication,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum HostEvent {
    Ready {
        protocol_version: u16,
    },
    Synced {
        revision: u64,
        fence_count: usize,
    },
    GeometryChanged {
        id: String,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        display_anchor: Option<DisplayAnchor>,
        placement: Option<FencePlacement>,
    },
    ImportFiles {
        fence_id: String,
        paths: Vec<PathBuf>,
    },
    UserAction {
        action: HostUserAction,
    },
    Notification {
        message: String,
    },
    DesktopVisibilityChanged {
        visible: bool,
    },
    Error {
        message: String,
    },
    Stopped,
}

pub fn message_line(message: &impl Serialize) -> Result<String, serde_json::Error> {
    serde_json::to_string(message).map(|mut encoded| {
        encoded.push('\n');
        encoded
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_keeps_paths_with_spaces_as_one_argument() {
        let args = vec![
            "creel.exe".into(),
            ARG_MAP_FOLDER.into(),
            r"C:\Users\Creel\Desktop\项目 资料".into(),
            ARG_SHOW.into(),
        ];
        assert_eq!(
            parse_external_commands(&args),
            vec![
                ExternalCommand::MapFolder(PathBuf::from(r"C:\Users\Creel\Desktop\项目 资料")),
                ExternalCommand::Show,
            ]
        );
    }

    #[test]
    fn parser_ignores_map_without_a_path() {
        assert!(parse_external_commands(&[ARG_MAP_FOLDER.into()]).is_empty());
    }

    #[test]
    fn parser_accepts_the_mapped_folder_shortcut() {
        assert_eq!(
            parse_external_commands(&[ARG_NEW_MAPPED_FENCE.into()]),
            vec![ExternalCommand::NewMappedBox]
        );
    }

    #[test]
    fn parser_accepts_explicit_quit_without_showing_the_window() {
        assert_eq!(
            parse_external_commands(&[ARG_QUIT.into()]),
            vec![ExternalCommand::Quit]
        );
    }

    #[test]
    fn host_protocol_round_trips_paths_and_revision() {
        let command = HostCommand::Sync {
            revision: 42,
            fences: vec![HostFenceSnapshot {
                id: "fence-1".into(),
                title: "项目资料".into(),
                directory: PathBuf::from(r"C:\Users\Creel\Desktop\项目 资料"),
                x: -120.5,
                y: 48.0,
                width: 330.0,
                height: 280.0,
                color: "sage".into(),
                content_color: "frosted".into(),
                collapsed: false,
                locked: true,
                display_anchor: None,
                placement: None,
            }],
            preferences: HostPreferencesSnapshot {
                title_opacity: 0.82,
                content_opacity: 0.64,
                show_fence_border: true,
                fence_border_opacity: 0.35,
                icon_size: 48,
                ghost_mode: false,
                ghost_mode_trigger: GhostModeTrigger::Automatic,
                ghost_opacity: 0.2,
                ghost_hotkey: "Ctrl+Alt+G".into(),
                show_hidden_files: true,
                show_fence_titles: true,
            },
            visible: true,
        };
        let line = message_line(&command).unwrap();
        assert!(line.ends_with('\n'));
        let decoded = serde_json::from_str::<HostCommand>(line.trim())
            .expect("host command should deserialize");
        assert_eq!(decoded, command);
    }

    #[test]
    fn host_geometry_event_round_trips_negative_coordinates() {
        let event = HostEvent::GeometryChanged {
            id: "fence-2".into(),
            x: -840.0,
            y: 72.0,
            width: 460.0,
            height: 330.0,
            display_anchor: Some(DisplayAnchor {
                device_name: r"\\.\DISPLAY1".into(),
                work_left: 0,
                work_top: 0,
                work_width: 1920,
                work_height: 1040,
                dpi_x: 96,
                dpi_y: 96,
                effective_dpi_x: Some(96),
                effective_dpi_y: Some(96),
            }),
            placement: Some(FencePlacement {
                group_id: "fence-2".into(),
                horizontal: LayoutAxis {
                    anchor: LayoutAnchor::End,
                    value: 0.0,
                },
                vertical: LayoutAxis {
                    anchor: LayoutAnchor::Start,
                    value: 12.0,
                },
                offset_x_dip: 0.0,
                offset_y_dip: 0.0,
            }),
        };
        let line = message_line(&event).unwrap();
        let decoded =
            serde_json::from_str::<HostEvent>(line.trim()).expect("host event should deserialize");
        assert_eq!(decoded, event);
    }

    #[test]
    fn host_notification_round_trips_unicode_messages() {
        let event = HostEvent::Notification {
            message: "无法打开桌面文件".into(),
        };
        let line = message_line(&event).unwrap();
        let decoded = serde_json::from_str::<HostEvent>(line.trim())
            .expect("notification should deserialize");
        assert_eq!(decoded, event);
    }

    #[test]
    fn host_visibility_event_round_trips() {
        let event = HostEvent::DesktopVisibilityChanged { visible: false };
        let line = message_line(&event).unwrap();
        let decoded = serde_json::from_str::<HostEvent>(line.trim())
            .expect("visibility event should deserialize");
        assert_eq!(decoded, event);
    }

    #[test]
    fn host_import_event_round_trips_unicode_paths() {
        let event = HostEvent::ImportFiles {
            fence_id: "fence-3".into(),
            paths: vec![
                PathBuf::from(r"C:\Users\Creel\Desktop\项目计划.docx"),
                PathBuf::from(r"D:\素材\图标.png"),
            ],
        };
        let line = message_line(&event).unwrap();
        let decoded = serde_json::from_str::<HostEvent>(line.trim())
            .expect("import event should deserialize");
        assert_eq!(decoded, event);
    }

    #[test]
    fn host_user_action_round_trips_rename_request() {
        let event = HostEvent::UserAction {
            action: HostUserAction::RenameFence {
                id: "fence-4".into(),
                title: "客户资料".into(),
            },
        };
        let line = message_line(&event).unwrap();
        let decoded =
            serde_json::from_str::<HostEvent>(line.trim()).expect("user action should deserialize");
        assert_eq!(decoded, event);
    }
}

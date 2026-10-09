use super::*;

fn message(id: &str) -> DesktopNotification {
    DesktopNotification {
        id: id.into(),
        kind: "message",
        title: String::new(),
        message: String::new(),
        version: None,
        action: None,
        shortcut_path: None,
    }
}

#[test]
fn timeout_is_bound_to_notification_and_render_generation() {
    let time = Instant::now();
    let mut state = NotificationState {
        current: Some(message("new")),
        shown_at: Some(time),
        ..Default::default()
    };
    assert!(!state.can_expire("old", time));
    assert!(state.can_expire("new", time));
    state.shown_at = None;
    assert!(!state.can_expire("new", time));
    state.shown_at = Some(time + Duration::from_secs(1));
    assert!(!state.can_expire("new", time));
    state.current.as_mut().unwrap().kind = "update";
    assert!(!state.can_expire("new", state.shown_at.unwrap()));
}

#[test]
fn shortcut_claim_requires_current_rendered_action_and_prevents_repeat() {
    let mut payload = message("new");
    payload.shortcut_path = Some(r"C:\test\broken.lnk".into());
    payload.action = Some("deleteShortcut");
    let mut state = NotificationState {
        current: Some(payload),
        ..Default::default()
    };
    assert!(state.claim_shortcut("new").is_err());
    state.shown_at = Some(Instant::now());
    assert!(state.claim_shortcut("old").is_err());
    assert_eq!(
        state.claim_shortcut("new").unwrap(),
        std::path::PathBuf::from(r"C:\test\broken.lnk")
    );
    assert!(state.claim_shortcut("new").is_err());
    // A page reload cannot claim a second operation for the same notification.
    state.shown_at = None;
    assert!(state.claim_shortcut("new").is_err());
    state.shown_at = Some(Instant::now());
    assert!(state.claim_shortcut("new").is_err());
    // An ordinary replacement never inherits the old delete action.
    state.current = Some(message("replacement"));
    assert!(state.claim_shortcut("new").is_err());
    assert!(state.claim_shortcut("replacement").is_err());
}

#[test]
fn serialized_action_never_exposes_a_deletable_path() {
    let mut payload = message("new");
    payload.shortcut_path = Some(r"C:\test\broken.lnk".into());
    payload.action = Some("deleteShortcut");
    let json = serde_json::to_value(payload).unwrap();
    assert_eq!(json["action"], "deleteShortcut");
    assert!(json.get("shortcutPath").is_none());
}

#[test]
fn delayed_completion_cannot_close_replacement_or_release_its_claim() {
    let mut state = NotificationState {
        current: Some(message("replacement")),
        deleting_id: Some("replacement".into()),
        ..Default::default()
    };
    assert!(!state.complete_shortcut("old"));
    assert_eq!(state.deleting_id.as_deref(), Some("replacement"));
    assert!(state.complete_shortcut("replacement"));
    assert!(state.deleting_id.is_none());
    state.current = None;
    assert!(!state.complete_shortcut("replacement"));
}

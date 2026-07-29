use tauri::{AppHandle, Runtime};
use tauri_plugin_autostart::ManagerExt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutostartAction {
    Enable,
    Disable,
    NoChange,
}

pub fn decide_autostart_action(desired: bool, actual: bool) -> AutostartAction {
    match (desired, actual) {
        (true, false) => AutostartAction::Enable,
        (false, true) => AutostartAction::Disable,
        _ => AutostartAction::NoChange,
    }
}

pub fn sync_autostart<R: Runtime>(app: &AppHandle<R>, desired: bool) -> Result<(), String> {
    let manager = app.autolaunch();
    let actual = manager
        .is_enabled()
        .map_err(|error| format!("failed to inspect the autostart registration: {error}"))?;

    match decide_autostart_action(desired, actual) {
        AutostartAction::Enable => manager
            .enable()
            .map_err(|error| format!("failed to enable autostart: {error}")),
        AutostartAction::Disable => manager
            .disable()
            .map_err(|error| format!("failed to disable autostart: {error}")),
        AutostartAction::NoChange => Ok(()),
    }
}

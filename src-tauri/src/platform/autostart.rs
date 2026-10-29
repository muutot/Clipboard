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

/// Persist a preference together with its native registration while the caller
/// retains the configuration lock.
pub fn persist_autostart<R: Runtime>(
    app: &AppHandle<R>,
    desired: bool,
    persist: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    let manager = app.autolaunch();
    let actual = manager
        .is_enabled()
        .map_err(|error| format!("failed to inspect the autostart registration: {error}"))?;
    persist_autostart_with(
        actual,
        desired,
        |enabled| {
            if enabled {
                manager
                    .enable()
                    .map_err(|error| format!("failed to enable autostart: {error}"))
            } else {
                manager
                    .disable()
                    .map_err(|error| format!("failed to disable autostart: {error}"))
            }
        },
        persist,
    )
}

fn persist_autostart_with(
    actual: bool,
    desired: bool,
    mut register: impl FnMut(bool) -> Result<(), String>,
    persist: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    if actual != desired {
        register(desired)?;
    }
    if let Err(error) = persist() {
        if actual != desired {
            if let Err(rollback) = register(actual) {
                return Err(format!(
                    "{error}; failed to restore autostart registration: {rollback}"
                ));
            }
        }
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod persistence_tests {
    use super::persist_autostart_with;
    use crate::config::ConfigStore;
    use std::cell::Cell;

    #[test]
    fn registration_failure_keeps_the_previous_preference_on_disk_and_in_memory() {
        let root =
            std::env::temp_dir().join(format!("clipboard-autostart-{}", uuid::Uuid::new_v4()));
        let mut config = ConfigStore::load(&root).unwrap();
        let result = persist_autostart_with(
            false,
            true,
            |_| Err("registration denied".into()),
            || {
                config
                    .set_launch_at_startup(true)
                    .map_err(|error| error.to_string())
            },
        );
        assert!(result.is_err());
        assert!(!config.launch_at_startup());
        assert!(!ConfigStore::load(&root).unwrap().launch_at_startup());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn save_failure_restores_the_actual_registration_even_if_it_differs_from_the_preference() {
        let root =
            std::env::temp_dir().join(format!("clipboard-autostart-{}", uuid::Uuid::new_v4()));
        let mut config = ConfigStore::load(&root).unwrap();
        // Simulate an externally enabled registration with a stored false preference.
        let actual = Cell::new(true);
        let calls = Cell::new(0);
        std::fs::rename(root.join("conf"), root.join("saved-conf")).unwrap();
        let result = persist_autostart_with(
            true,
            false,
            |enabled| {
                actual.set(enabled);
                calls.set(calls.get() + 1);
                Ok(())
            },
            || {
                config
                    .set_launch_at_startup(false)
                    .map_err(|error| error.to_string())
            },
        );
        assert!(result.is_err());
        assert!(actual.get());
        assert_eq!(calls.get(), 2, "failed save must undo the native update");
        assert!(!config.launch_at_startup());
        std::fs::rename(root.join("saved-conf"), root.join("conf")).unwrap();
        assert!(!ConfigStore::load(&root).unwrap().launch_at_startup());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn successful_updates_and_no_change_both_persist_the_preference() {
        for initial in [false, true] {
            let root =
                std::env::temp_dir().join(format!("clipboard-autostart-{}", uuid::Uuid::new_v4()));
            let mut config = ConfigStore::load(&root).unwrap();
            let actual = Cell::new(initial);
            let calls = Cell::new(0);
            persist_autostart_with(
                initial,
                true,
                |enabled| {
                    actual.set(enabled);
                    calls.set(calls.get() + 1);
                    Ok(())
                },
                || {
                    config
                        .set_launch_at_startup(true)
                        .map_err(|error| error.to_string())
                },
            )
            .unwrap();
            assert!(actual.get());
            assert_eq!(calls.get(), usize::from(!initial));
            assert!(ConfigStore::load(&root).unwrap().launch_at_startup());
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn failed_native_rollback_is_reported_with_the_original_save_error() {
        let calls = Cell::new(0);
        let error = persist_autostart_with(
            false,
            true,
            |_| {
                calls.set(calls.get() + 1);
                if calls.get() == 1 {
                    Ok(())
                } else {
                    Err("rollback denied".into())
                }
            },
            || Err("save denied".into()),
        )
        .unwrap_err();
        assert!(error.contains("save denied"));
        assert!(error.contains("rollback denied"));
    }
}

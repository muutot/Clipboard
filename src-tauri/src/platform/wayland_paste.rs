//! Conditional quick paste for Sway/Hyprland with the compositor's own focus
//! command and wtype. Other Wayland desktops return an explicit unsupported error.
use super::quick_paste::Target;
use std::{
    process::Command,
    thread,
    time::{Duration, Instant},
};

fn command(program: &str, args: &[&str]) -> Result<Vec<u8>, String> {
    use super::bounded_command::BoundedCommandExt;
    let output = Command::new(program)
        .args(args)
        .bounded_output(1024 * 1024)
        .map_err(|error| format!("{program}: {error}"))?;
    if !output.status.success() {
        return Err(format!("{program} failed"));
    }
    Ok(output.stdout)
}

fn has_command(name: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|p| p.join(name).is_file()))
}
fn sway() -> bool {
    std::env::var_os("SWAYSOCK").is_some_and(|v| !v.is_empty())
}
fn hyprland() -> bool {
    std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some_and(|v| !v.is_empty())
}
pub fn available() -> bool {
    has_command("wtype")
        && ((sway() && has_command("swaymsg")) || (hyprland() && has_command("hyprctl")))
}

fn sway_target(node: &serde_json::Value) -> Option<Target> {
    if node.get("focused").and_then(|v| v.as_bool()) == Some(true) {
        if let (Some(handle), Some(pid)) = (
            node.get("id").and_then(|v| v.as_i64()),
            node.get("pid").and_then(|v| v.as_u64()),
        ) {
            return Some(Target {
                handle: isize::try_from(handle).ok()?,
                pid: u32::try_from(pid).ok()?,
            });
        }
    }
    for key in ["nodes", "floating_nodes"] {
        if let Some(nodes) = node.get(key).and_then(|v| v.as_array()) {
            if let Some(target) = nodes.iter().find_map(sway_target) {
                return Some(target);
            }
        }
    }
    None
}
pub fn current() -> Option<Target> {
    let target = if sway() {
        sway_target(
            &serde_json::from_slice(&command("swaymsg", &["-t", "get_tree", "-r"]).ok()?).ok()?,
        )?
    } else if hyprland() {
        let value: serde_json::Value =
            serde_json::from_slice(&command("hyprctl", &["activewindow", "-j"]).ok()?).ok()?;
        let address = value.get("address")?.as_str()?.strip_prefix("0x")?;
        Target {
            handle: isize::from_str_radix(address, 16).ok()?,
            pid: u32::try_from(value.get("pid")?.as_u64()?).ok()?,
        }
    } else {
        return None;
    };
    (target.handle > 0 && target.pid > 0 && target.pid != std::process::id()).then_some(target)
}
pub fn paste(target: Target) -> Result<(), String> {
    if !available() {
        return Err("Wayland quick paste requires Sway or Hyprland and wtype".into());
    }
    if target.handle <= 0 || target.pid == 0 || target.pid == std::process::id() {
        return Err("invalid paste target".into());
    }
    // Revalidate the pid after activation so a reused compositor id cannot
    // receive the clipboard contents intended for an old window.
    if sway() {
        let reply = command("swaymsg", &[&format!("[con_id={}] focus", target.handle)])?;
        let results: serde_json::Value =
            serde_json::from_slice(&reply).map_err(|e| e.to_string())?;
        if !results
            .as_array()
            .is_some_and(|a| !a.is_empty() && a.iter().all(|v| v["success"] == true))
        {
            return Err("Sway declined window activation".into());
        }
    } else {
        command(
            "hyprctl",
            &[
                "dispatch",
                "focuswindow",
                &format!("address:0x{:x}", target.handle),
            ],
        )?;
    }
    let deadline = Instant::now() + Duration::from_millis(600);
    while current() != Some(target) {
        if Instant::now() >= deadline {
            return Err("previous Wayland window did not regain focus".into());
        }
        thread::sleep(Duration::from_millis(20));
    }
    // wtype owns and releases the synthetic Control modifier in one process.
    command("wtype", &["-M", "ctrl", "-k", "v", "-m", "ctrl"])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn finds_focused_floating_leaf_and_ignores_containers_without_pid() {
        let tree = serde_json::json!({"focused":true,"id":1,"nodes":[],"floating_nodes":[{"focused":true,"id":99,"pid":123}]});
        assert_eq!(
            sway_target(&tree),
            Some(Target {
                handle: 99,
                pid: 123
            })
        );
        assert!(sway_target(&serde_json::json!({"focused":false,"id":2,"pid":456})).is_none());
    }
}

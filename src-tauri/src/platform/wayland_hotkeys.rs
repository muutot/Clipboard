//! GlobalShortcuts portal integration. Bare modifier taps are not a portal
//! capability and must never be advertised as available on Wayland.
use crate::{
    keyboard::Modifier,
    platform::{
        hotkey_common::HotkeyRegistration,
        windows_hotkey::{action_for_hotkey_id, HotkeyAction},
    },
};
use dbus::{
    arg::{PropMap, Variant},
    blocking::Connection,
    message::MatchRule,
    Path,
};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread,
    time::Duration,
};
use tauri::Emitter;

pub static READY: AtomicBool = AtomicBool::new(false);
const SERVICE: &str = "org.freedesktop.portal.Desktop";
const DESKTOP: &str = "/org/freedesktop/portal/desktop";
const INTERFACE: &str = "org.freedesktop.portal.GlobalShortcuts";

fn options() -> PropMap {
    let mut options = PropMap::new();
    options.insert(
        "handle_token".into(),
        Variant(Box::new(format!(
            "clipboard{}",
            uuid::Uuid::new_v4().simple()
        ))),
    );
    options
}

struct Session<'a> {
    conn: &'a Connection,
    path: Path<'static>,
}
impl Drop for Session<'_> {
    fn drop(&mut self) {
        READY.store(false, Ordering::SeqCst);
        let _: Result<(), _> = self
            .conn
            .with_proxy(SERVICE, self.path.clone(), Duration::from_millis(200))
            .method_call("org.freedesktop.portal.Session", "Close", ());
    }
}

type Response = (String, u32, PropMap);
fn response(
    conn: &Connection,
    rx: &mpsc::Receiver<Response>,
    request: Path<'static>,
    stop: &AtomicBool,
) -> Result<PropMap, String> {
    loop {
        if stop.load(Ordering::SeqCst) {
            let _: Result<(), _> = conn
                .with_proxy(SERVICE, request, Duration::from_millis(200))
                .method_call("org.freedesktop.portal.Request", "Close", ());
            return Err("shortcut request cancelled".into());
        }
        conn.process(Duration::from_millis(50))
            .map_err(|e| e.to_string())?;
        while let Ok((path, status, values)) = rx.try_recv() {
            if path == request.to_string() {
                return if status == 0 {
                    Ok(values)
                } else {
                    Err(format!("shortcut portal rejected request ({status})"))
                };
            }
        }
    }
}

pub fn start(
    registrations: Vec<HotkeyRegistration>,
    doubles: Vec<Modifier>,
    tx: mpsc::Sender<HotkeyAction>,
    app: tauri::AppHandle,
    stop: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        READY.store(false, Ordering::SeqCst);
        if !doubles.is_empty() {
            let _ = app.emit("hotkey-registration-failed", serde_json::json!({"action":"toggleWindow", "error":"Wayland does not expose bare modifier double taps; configure a key combination"}));
        }
        if registrations.is_empty() {
            return;
        }
        let result = run(registrations, tx, &stop);
        if let Err(error) = result {
            if !stop.load(Ordering::SeqCst) {
                crate::log_warn!("[hotkey] Wayland portal: {error}");
                let _ = app.emit(
                    "hotkey-registration-failed",
                    serde_json::json!({"action":"toggleWindow", "error":error}),
                );
            }
        }
    })
}

fn run(
    registrations: Vec<HotkeyRegistration>,
    tx: mpsc::Sender<HotkeyAction>,
    stop: &AtomicBool,
) -> Result<(), String> {
    let conn = Connection::new_session().map_err(|e| e.to_string())?;
    let proxy = conn.with_proxy(SERVICE, DESKTOP, Duration::from_secs(2));
    let (response_tx, response_rx) = mpsc::channel();
    conn.add_match(
        MatchRule::new_signal("org.freedesktop.portal.Request", "Response").with_sender(SERVICE),
        move |(status, values): (u32, PropMap), _, message| {
            if let Some(path) = message.path() {
                let _ = response_tx.send((path.to_string(), status, values));
            }
            true
        },
    )
    .map_err(|e| e.to_string())?;
    let mut create_options = options();
    create_options.insert(
        "session_handle_token".into(),
        Variant(Box::new(format!(
            "clipboard{}",
            uuid::Uuid::new_v4().simple()
        ))),
    );
    let (request,): (Path<'static>,) = proxy
        .method_call(INTERFACE, "CreateSession", (create_options,))
        .map_err(|e| e.to_string())?;
    let values = response(&conn, &response_rx, request, stop)?;
    let session_path = values
        .get("session_handle")
        .and_then(|v| v.0.as_str())
        .ok_or("portal omitted session handle")?;
    let session = Session {
        conn: &conn,
        path: Path::new(session_path.to_owned()).map_err(|e| e.to_string())?,
    };
    let (closed_tx, closed_rx) = mpsc::channel();
    conn.add_match(
        MatchRule::new_signal("org.freedesktop.portal.Session", "Closed")
            .with_sender(SERVICE)
            .with_path(session.path.clone()),
        move |(): (), _, _| {
            let _ = closed_tx.send(());
            true
        },
    )
    .map_err(|e| e.to_string())?;
    let expected_session = session.path.to_string();
    let ids: std::collections::HashMap<String, i32> = registrations
        .iter()
        .map(|(id, _, _)| (format!("clipboard_{id}"), *id))
        .collect();
    conn.add_match(
        MatchRule::new_signal(INTERFACE, "Activated").with_sender(SERVICE),
        move |(path, id, _, _): (Path<'static>, String, u64, PropMap), _, _| {
            if path.to_string() == expected_session {
                if let Some(id) = ids.get(&id) {
                    let _ = tx.send(action_for_hotkey_id(*id));
                }
            }
            true
        },
    )
    .map_err(|e| e.to_string())?;
    let shortcuts: Vec<(String, PropMap)> = registrations
        .iter()
        .map(|(id, modifiers, key)| {
            let mut properties = PropMap::new();
            let action_index =
                crate::platform::hotkey_common::action_index_for_hotkey_id(*id).unwrap_or(0);
            let action = crate::keyboard::global_action_ids()
                .nth(action_index)
                .unwrap_or("Clipboard");
            properties.insert(
                "description".into(),
                Variant(Box::new(format!("Clipboard: {action}"))),
            );
            if let Some(trigger) = trigger(*modifiers, *key) {
                properties.insert("preferred_trigger".into(), Variant(Box::new(trigger)));
            }
            (format!("clipboard_{id}"), properties)
        })
        .collect();
    let (request,): (Path<'static>,) = proxy
        .method_call(
            INTERFACE,
            "BindShortcuts",
            (session.path.clone(), shortcuts, "", options()),
        )
        .map_err(|e| e.to_string())?;
    let bound = response(&conn, &response_rx, request, stop)?;
    if !bound
        .get("shortcuts")
        .and_then(|value| value.0.as_iter())
        .is_some_and(|mut values| values.next().is_some())
    {
        return Err("portal did not bind any shortcuts".into());
    }
    READY.store(true, Ordering::SeqCst);
    while !stop.load(Ordering::SeqCst) && closed_rx.try_recv().is_err() {
        conn.process(Duration::from_millis(50))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn trigger(modifiers: u32, key: u32) -> Option<String> {
    let key = match key {
        0x30..=0x39 | 0x41..=0x5a => char::from_u32(key)?.to_ascii_lowercase().to_string(),
        0x70..=0x87 => format!("F{}", key - 0x6f),
        0x08 => "BackSpace".into(),
        0x09 => "Tab".into(),
        0x0d => "Return".into(),
        0x1b => "Escape".into(),
        0x20 => "space".into(),
        0x21 => "Prior".into(),
        0x22 => "Next".into(),
        0x23 => "End".into(),
        0x24 => "Home".into(),
        0x25 => "Left".into(),
        0x26 => "Up".into(),
        0x27 => "Right".into(),
        0x28 => "Down".into(),
        0x2d => "Insert".into(),
        0x2e => "Delete".into(),
        _ => return None,
    };
    let mut result = String::new();
    for (bit, name) in [(1, "ALT+"), (2, "CTRL+"), (4, "SHIFT+"), (8, "LOGO+")] {
        if modifiers & bit != 0 {
            result.push_str(name);
        }
    }
    result.push_str(&key);
    Some(result)
}

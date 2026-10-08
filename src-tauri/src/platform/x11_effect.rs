//! KWin-compatible blur hint. Rendering remains the compositor's decision.
use x11rb::{
    connection::Connection,
    protocol::xproto::{AtomEnum, ConnectionExt, PropMode},
    wrapper::ConnectionExt as _,
};
pub fn set_blur(window: u32, enabled: bool) -> Result<(), String> {
    let (conn, _) = x11rb::connect(None).map_err(|e| e.to_string())?;
    let atom = conn
        .intern_atom(false, b"_KDE_NET_WM_BLUR_BEHIND_REGION")
        .map_err(|e| e.to_string())?
        .reply()
        .map_err(|e| e.to_string())?
        .atom;
    if enabled {
        // A zero-length CARDINAL region requests blur for the whole window.
        conn.change_property32(PropMode::REPLACE, window, atom, AtomEnum::CARDINAL, &[])
            .map_err(|e| e.to_string())?
            .check()
            .map_err(|e| e.to_string())?;
    } else {
        conn.delete_property(window, atom)
            .map_err(|e| e.to_string())?
            .check()
            .map_err(|e| e.to_string())?;
    }
    conn.flush().map_err(|e| e.to_string())
}

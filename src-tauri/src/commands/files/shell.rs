//! OS integration commands: opening external URLs and revealing files.

/// Returns true when the URL uses a scheme the OS opener may be given.
/// `http(s)` covers web links; `mailto`/`tel` let the detected email/phone
/// quick actions reach the OS handler. Everything else (`file://`, bare
/// paths, `javascript:`) stays rejected because the text can come from
/// clipboard content.
pub(super) fn openable_scheme(url: &str) -> bool {
    let trimmed = url.trim();
    ["http://", "https://", "mailto:", "tel:"]
        .iter()
        .any(|scheme| {
            trimmed
                .get(..scheme.len())
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case(scheme))
        })
}

#[tauri::command]
pub fn open_external_url(url: String) -> Result<(), String> {
    let trimmed = url.trim();
    if !openable_scheme(trimmed) {
        return Err("only http(s), mailto, and tel URLs can be opened".to_string());
    }
    open::that(trimmed).map_err(|e| format!("failed to open URL: {e}"))
}

#[tauri::command]
pub fn reveal_in_explorer(path: String) -> Result<(), String> {
    let p = std::path::Path::new(&path);
    if !p.exists() {
        return Err("file not found".to_string());
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .args(["/select,", &path])
            .spawn()
            .map_err(|e| format!("explorer: {e}"))?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        open::that(p.parent().unwrap_or(p)).map_err(|e| format!("open: {e}"))?;
    }
    Ok(())
}

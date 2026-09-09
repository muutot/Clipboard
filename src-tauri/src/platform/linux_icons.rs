//! Linux application icon lookup via freedesktop `.desktop` files.
//!
//! The X11/Wayland adapters cannot extract icons from running processes, so
//! this module resolves them statically: match the executable against
//! `Exec=` entries in the XDG application directories, then resolve the
//! `Icon=` value through the XDG icon directories. All lookup logic is pure
//! over explicit directories (unit-tested with fixtures on every platform);
//! only [`linux_data_dirs`] and the thin adapter wrappers touch the real
//! filesystem layout and environment.

use std::path::{Path, PathBuf};

/// XDG data directories searched for `applications/*.desktop` and icon
/// themes. Honors `XDG_DATA_HOME`/`XDG_DATA_DIRS`, falling back to the
/// spec defaults.
pub fn linux_data_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from) {
        dirs.push(home);
    } else if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        dirs.push(home.join(".local/share"));
    }
    if let Some(dirs_env) = std::env::var_os("XDG_DATA_DIRS") {
        dirs.extend(std::env::split_paths(&dirs_env));
    } else {
        dirs.push(PathBuf::from("/usr/local/share"));
        dirs.push(PathBuf::from("/usr/share"));
    }
    dirs
}

/// Parsed `[Desktop Entry]` fields relevant to icon resolution.
#[derive(Debug, PartialEq, Eq)]
struct DesktopEntry {
    exec_binary: String,
    name: String,
    icon: String,
}

/// Parses the `[Desktop Entry]` section of one `.desktop` file. Returns
/// `None` when `Exec=` or `Icon=` is absent.
fn parse_desktop_entry(path: &Path) -> Option<DesktopEntry> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut in_entry = false;
    let mut exec = None;
    let mut name = None;
    let mut icon = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry {
            continue;
        }
        let (key, value) = line.split_once('=')?;
        match key.trim() {
            "Exec" if exec.is_none() => exec = Some(value.trim().to_owned()),
            "Name" if name.is_none() => name = Some(value.trim().to_owned()),
            "Icon" if icon.is_none() => icon = Some(value.trim().to_owned()),
            _ => {}
        }
    }
    Some(DesktopEntry {
        exec_binary: first_exec_token(&exec?)?,
        name: name?,
        icon: icon?,
    })
}

/// First token of an `Exec=` line with surrounding quotes stripped, e.g.
/// `"/usr/bin/foo" --flag %U` → `/usr/bin/foo`.
fn first_exec_token(exec: &str) -> Option<String> {
    let token = exec.split_whitespace().next()?.trim();
    if token.is_empty() {
        return None;
    }
    Some(token.trim_matches('"').trim_matches('\'').to_owned())
}

fn binary_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// Finds the on-disk icon for an executable by scanning `.desktop` files.
/// Matches `Exec=` binaries first (basename, case-insensitive), then falls
/// back to `Name=` matching `app_name`. Returns the resolved icon file path.
pub fn find_app_icon_source(
    data_dirs: &[PathBuf],
    exe_path: &str,
    app_name: &str,
) -> Option<PathBuf> {
    let exe_base = binary_name(exe_path);
    let mut name_fallback: Option<PathBuf> = None;
    for data_dir in data_dirs {
        let applications = data_dir.join("applications");
        let Ok(dir) = std::fs::read_dir(&applications) else {
            continue;
        };
        let mut files: Vec<PathBuf> = dir
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "desktop"))
            .collect();
        files.sort();
        for file in files {
            let Some(entry) = parse_desktop_entry(&file) else {
                continue;
            };
            if !exe_base.is_empty()
                && binary_name(&entry.exec_binary).eq_ignore_ascii_case(exe_base)
            {
                if let Some(icon) = resolve_icon_value(data_dirs, &entry.icon) {
                    return Some(icon);
                }
            }
            if name_fallback.is_none()
                && !app_name.is_empty()
                && entry.name.eq_ignore_ascii_case(app_name)
            {
                name_fallback = resolve_icon_value(data_dirs, &entry.icon);
            }
        }
    }
    name_fallback
}

/// Resolves an `Icon=` value: absolute paths pass through when they exist,
/// otherwise theme directories are searched (vector preferred).
fn resolve_icon_value(data_dirs: &[PathBuf], icon: &str) -> Option<PathBuf> {
    let path = Path::new(icon);
    if path.is_absolute() {
        return path.is_file().then(|| path.to_path_buf());
    }
    let icon_dirs: Vec<PathBuf> = data_dirs.iter().map(|dir| dir.join("icons")).collect();
    // Vector first so tray/list rendering stays crisp at any size.
    for extension in ["svg", "png", "xpm"] {
        let file_name = if icon.ends_with(&format!(".{extension}")) {
            icon.to_owned()
        } else {
            format!("{icon}.{extension}")
        };
        let mut hits: Vec<PathBuf> = icon_dirs
            .iter()
            .flat_map(|dir| find_named_file(dir, &file_name, 8))
            .collect();
        hits.sort();
        if let Some(first) = hits.into_iter().next() {
            return Some(first);
        }
    }
    None
}

/// Resolves the icon for a foreground app and stages it into the managed
/// icons directory, returning the cached file name. A previously cached file
/// is reused without scanning. The file keeps its source extension under the
/// shared `icon_key(app)` stem so cache listing (which understands
/// png/ico/svg/jpg/jpeg) keeps working.
pub fn ensure_cached_app_icon(icons_dir: &Path, app_name: &str, exe_path: &str) -> Option<String> {
    let key = crate::content::icon_key(app_name);
    if key.is_empty() {
        return None;
    }
    for extension in ["png", "svg", "xpm", "ico", "jpg", "jpeg"] {
        let name = format!("{key}.{extension}");
        if icons_dir.join(&name).is_file() {
            return Some(name);
        }
    }
    let source = find_app_icon_source(&linux_data_dirs(), exe_path, app_name)?;
    let extension = source
        .extension()
        .and_then(|ext| ext.to_str())
        .filter(|ext| !ext.is_empty())
        .unwrap_or("png");
    let name = format!("{key}.{extension}");
    std::fs::create_dir_all(icons_dir).ok()?;
    std::fs::copy(&source, icons_dir.join(&name)).ok()?;
    Some(name)
}

/// Depth-limited recursive search for one file name. Symlinked directories
/// are not descended into, so theme symlink farms cannot loop forever.
fn find_named_file(dir: &Path, file_name: &str, max_depth: usize) -> Vec<PathBuf> {
    let mut hits = Vec::new();
    let mut stack = vec![(dir.to_path_buf(), 0)];
    while let Some((current, depth)) = stack.pop() {
        if depth > max_depth {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() && !file_type.is_symlink() {
                stack.push((path, depth + 1));
            } else if file_type.is_file() && path.file_name().is_some_and(|n| n == file_name) {
                hits.push(path);
            }
        }
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new(label: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "linux-icons-test-{label}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(root.join("applications")).unwrap();
            std::fs::create_dir_all(root.join("icons/hicolor/48x48/apps")).unwrap();
            Self { root }
        }

        fn desktop(&self, name: &str, body: &str) {
            std::fs::write(self.root.join("applications").join(name), body).unwrap();
        }

        fn icon(&self, relative: &str) {
            let path = self.root.join("icons").join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"fake-icon-bytes").unwrap();
        }

        fn data_dirs(&self) -> Vec<PathBuf> {
            vec![self.root.clone()]
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn exec_match_resolves_themed_icon() {
        let fixture = Fixture::new("exec");
        fixture.desktop(
            "firefox.desktop",
            "[Desktop Entry]\nName=Firefox\nExec=/usr/bin/firefox %u\nIcon=firefox\n",
        );
        fixture.icon("hicolor/48x48/apps/firefox.png");
        let resolved = find_app_icon_source(&fixture.data_dirs(), "/usr/bin/firefox", "Firefox");
        assert_eq!(
            resolved,
            Some(fixture.root.join("icons/hicolor/48x48/apps/firefox.png"))
        );
    }

    #[test]
    fn name_match_falls_back_when_exec_differs() {
        let fixture = Fixture::new("name");
        fixture.desktop(
            "wrapper.desktop",
            "[Desktop Entry]\nName=MyApp\nExec=/opt/myapp/launcher.sh --flag %U\nIcon=myapp\n",
        );
        fixture.icon("hicolor/48x48/apps/myapp.svg");
        // Unrelated running binary, but the app name matches Name=.
        let resolved =
            find_app_icon_source(&fixture.data_dirs(), "/usr/bin/totally-other", "MyApp");
        assert_eq!(
            resolved,
            Some(fixture.root.join("icons/hicolor/48x48/apps/myapp.svg"))
        );
    }

    #[test]
    fn absolute_icon_paths_pass_through_and_missing_icons_yield_none() {
        let fixture = Fixture::new("absolute");
        let absolute = fixture.root.join("icons/custom.png");
        fixture.icon("custom.png");
        fixture.desktop(
            "custom.desktop",
            format!(
                "[Desktop Entry]\nName=Custom\nExec=custom-bin\nIcon={}\n",
                absolute.display()
            )
            .as_str(),
        );
        let resolved = find_app_icon_source(&fixture.data_dirs(), "/usr/bin/custom-bin", "Custom");
        assert_eq!(resolved, Some(absolute));
        assert_eq!(
            find_app_icon_source(&fixture.data_dirs(), "/usr/bin/unknown", "Unknown"),
            None
        );
    }

    #[test]
    fn entries_without_exec_or_icon_are_skipped() {
        let fixture = Fixture::new("sparse");
        fixture.desktop(
            "noexec.desktop",
            "[Desktop Entry]\nName=NoExec\nIcon=noexec\n",
        );
        fixture.desktop(
            "noicon.desktop",
            "[Desktop Entry]\nName=NoIcon\nExec=noicon-bin\n",
        );
        assert_eq!(
            find_app_icon_source(&fixture.data_dirs(), "/usr/bin/noexec-bin", "NoExec"),
            None
        );
        assert_eq!(
            find_app_icon_source(&fixture.data_dirs(), "/usr/bin/noicon-bin", "NoIcon"),
            None
        );
    }

    #[test]
    fn svg_is_preferred_over_raster() {
        let fixture = Fixture::new("vector");
        fixture.desktop(
            "app.desktop",
            "[Desktop Entry]\nName=App\nExec=app-bin\nIcon=app\n",
        );
        fixture.icon("hicolor/48x48/apps/app.png");
        fixture.icon("hicolor/scalable/apps/app.svg");
        let resolved = find_app_icon_source(&fixture.data_dirs(), "/usr/bin/app-bin", "App");
        assert_eq!(
            resolved,
            Some(fixture.root.join("icons/hicolor/scalable/apps/app.svg"))
        );
    }

    #[test]
    fn ensure_cached_app_icon_stages_and_reuses() {
        let fixture = Fixture::new("cache");
        fixture.desktop(
            "app.desktop",
            "[Desktop Entry]\nName=App\nExec=app-bin\nIcon=app\n",
        );
        fixture.icon("hicolor/48x48/apps/app.png");
        // Point XDG at the fixture so no real system directories are touched.
        std::env::set_var("XDG_DATA_HOME", fixture.root.as_os_str());
        std::env::set_var("XDG_DATA_DIRS", "");
        let icons_dir = fixture.root.join("cache-icons");

        let first = ensure_cached_app_icon(&icons_dir, "App", "/usr/bin/app-bin");
        assert!(first.is_some());
        let staged = icons_dir.join(first.unwrap());
        assert!(staged.is_file());

        // Second call reuses the staged file even when sources vanish.
        std::fs::remove_dir_all(fixture.root.join("applications")).unwrap();
        std::fs::remove_dir_all(fixture.root.join("icons")).unwrap();
        let second = ensure_cached_app_icon(&icons_dir, "App", "/usr/bin/app-bin");
        assert_eq!(second.map(|name| icons_dir.join(name)), Some(staged));

        std::env::remove_var("XDG_DATA_HOME");
        std::env::remove_var("XDG_DATA_DIRS");
    }
}

//! Linux support: the X11 and Wayland clipboard backends plus the helpers
//! they share — freedesktop icon lookup, subprocess stop pipes, and
//! `text/uri-list` parsing.

pub mod icons;
#[cfg(target_os = "linux")]
pub mod stop_pipe;

#[cfg(target_os = "linux")]
pub mod x11;
#[cfg(not(target_os = "linux"))]
#[path = "x11/mod.rs"]
pub mod x11;

#[cfg(target_os = "linux")]
pub mod wayland;
#[cfg(not(target_os = "linux"))]
#[path = "wayland/mod.rs"]
pub mod wayland;

/// Parses `text/uri-list` clipboard output into local file paths.
///
/// Skips blank lines and `#` comments and keeps only `file://` entries.
/// Shared by the X11 (`xclip`) and Wayland (`wl-paste`) readers so both
/// backends agree on edge cases.
///
/// An empty or `localhost` authority maps to the local path; any other host
/// is skipped because it names a remote machine, not a local file.
/// Percent-encoding is decoded (`%20` to space); undecodable sequences fall
/// back to the raw path rather than dropping the entry.
pub fn parse_uri_list(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let rest = line.strip_prefix("file://")?;
            let path = if rest.starts_with('/') {
                rest
            } else {
                match rest.find('/') {
                    Some(index) => {
                        let (host, path) = rest.split_at(index);
                        if host.is_empty() || host.eq_ignore_ascii_case("localhost") {
                            path
                        } else {
                            return None;
                        }
                    }
                    None => return None,
                }
            };
            if path.is_empty() {
                return None;
            }
            Some(
                urlencoding::decode(path)
                    .map(|decoded| decoded.into_owned())
                    .unwrap_or_else(|_| path.to_owned()),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn parse_uri_list_keeps_only_file_entries() {
        let parsed = super::parse_uri_list(        "# comment\n\nfile:///home/user/a.txt\n  file:///home/user/b.txt  \nhttp://example.com/x\ntext/plain\n",    );
        assert_eq!(parsed, vec!["/home/user/a.txt", "/home/user/b.txt"]);
    }
    #[test]
    fn parse_uri_list_empty_input_yields_no_paths() {
        assert!(super::parse_uri_list("").is_empty());
        assert!(super::parse_uri_list("# only a comment\n   \n").is_empty());
    }
    #[test]
    fn parse_uri_list_decodes_percent_encoding() {
        let parsed = super::parse_uri_list("file:///home/user/my%20doc.txt\n");
        assert_eq!(parsed, vec!["/home/user/my doc.txt"]);
    }
    #[test]
    fn parse_uri_list_accepts_localhost_and_skips_remote_hosts() {
        let parsed = super::parse_uri_list(
            "file://localhost/home/user/a.txt\nfile://otherhost/home/user/b.txt\nfile://host\n",
        );
        assert_eq!(parsed, vec!["/home/user/a.txt"]);
    }
}

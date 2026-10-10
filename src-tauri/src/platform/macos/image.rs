//! macOS image-tool orchestration; native commands use the shared capture budget.
use std::{
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    process::{Command, Output},
};

#[cfg(target_os = "macos")]
use super::bounded_command::BoundedCommandExt;

const MAX_IMAGE_BYTES: usize = 64 * 1024 * 1024;
const MAX_TOOL_TEXT: usize = 64 * 1024;
type Runner<'a> = dyn FnMut(&mut Command, usize) -> io::Result<Output> + 'a;

struct Scratch(PathBuf);
impl Scratch {
    fn create(parent: &Path) -> io::Result<Self> {
        let path = parent.join(format!("clipboard-image-{}", uuid::Uuid::new_v4()));
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            fs::DirBuilder::new().mode(0o700).create(&path)?;
        }
        #[cfg(not(unix))]
        fs::create_dir(&path)?;
        Ok(Self(path))
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        // Only our two known files and empty private directory may be removed.
        let _ = fs::remove_file(self.0.join("clipboard.tiff"));
        let _ = fs::remove_file(self.0.join("clipboard.png"));
        let _ = fs::remove_dir(&self.0);
    }
}

fn validate_file_size(path: &Path) -> io::Result<()> {
    let metadata = fs::metadata(path)?;
    if !metadata.is_file() || metadata.len() > MAX_IMAGE_BYTES as u64 {
        super::bounded_command::fail_capture();
        return Err(io::Error::other("image tool file exceeds limit"));
    }
    Ok(())
}

fn read_image_file(path: &Path) -> io::Result<Vec<u8>> {
    validate_file_size(path)?;
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(MAX_IMAGE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_IMAGE_BYTES {
        super::bounded_command::fail_capture();
        return Err(io::Error::other("image tool file grew past limit"));
    }
    Ok(bytes)
}

fn decode(bytes: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    let rgba = crate::content::hash::decode_image_bytes(bytes)?.to_rgba8();
    let (width, height) = rgba.dimensions();
    Some((rgba.into_raw(), width, height))
}

#[cfg(target_os = "macos")]
pub fn read_clipboard_image() -> Option<(Vec<u8>, u32, u32)> {
    read_with(&std::env::temp_dir(), &mut |command, limit| {
        command.bounded_output(limit)
    })
}

fn read_with(parent: &Path, run: &mut Runner<'_>) -> Option<(Vec<u8>, u32, u32)> {
    for (tool, args) in [("pngpaste", &["-"][..]), ("imgpaste", &[][..])] {
        if super::bounded_command::capture_aborted() {
            return None;
        }
        if let Ok(output) = run(Command::new(tool).args(args), MAX_IMAGE_BYTES) {
            if output.status.success() {
                if let Some(image) = decode(&output.stdout) {
                    return Some(image);
                }
            }
        }
    }
    if super::bounded_command::capture_aborted() {
        return None;
    }
    let scratch = Scratch::create(parent).ok()?;
    let tiff = scratch.0.join("clipboard.tiff");
    let png = scratch.0.join("clipboard.png");
    let escaped = tiff
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let script = format!(
        "set img to (the clipboard as picture)\n\
         set fileRef to open for access POSIX file \"{escaped}\" with write permission\n\
         write img to fileRef\n\
         close access fileRef"
    );
    let output = run(
        Command::new("osascript")
            .current_dir(&scratch.0)
            .args(["-e", &script]),
        MAX_TOOL_TEXT,
    )
    .ok()?;
    if !output.status.success() {
        return None;
    }
    validate_file_size(&tiff).ok()?;
    convert(&tiff, &png, run).ok()?;
    decode(&read_image_file(&png).ok()?)
}

fn convert(source: &Path, target: &Path, run: &mut Runner<'_>) -> io::Result<()> {
    let output = run(
        Command::new("sips")
            .args(["-s", "format", "png"])
            .arg(source)
            .arg("--out")
            .arg(target),
        MAX_TOOL_TEXT,
    )?;
    if !output.status.success() {
        return Err(io::Error::other("image conversion failed"));
    }
    validate_file_size(target)
}

#[cfg(target_os = "macos")]
pub fn convert_icon(source: &Path, target: &Path) -> Option<()> {
    convert_icon_with(source, target, &mut |command, limit| {
        command.bounded_output(limit)
    })
}

fn convert_icon_with(source: &Path, target: &Path, run: &mut Runner<'_>) -> Option<()> {
    crate::content::file_store::store_atomically(target, |temporary| {
        convert(source, temporary, run)?;
        if decode(&read_image_file(temporary)?).is_none() {
            return Err(io::Error::other("invalid converted icon"));
        }
        Ok(())
    })
    .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::ExitStatus;

    fn output(bytes: Vec<u8>) -> Output {
        #[cfg(unix)]
        use std::os::unix::process::ExitStatusExt;
        #[cfg(windows)]
        use std::os::windows::process::ExitStatusExt;
        Output {
            status: ExitStatus::from_raw(0),
            stdout: bytes,
            stderr: Vec::new(),
        }
    }
    fn png() -> Vec<u8> {
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(2, 1, image::Rgba([12, 34, 56, 255]))
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    }
    fn parent() -> Scratch {
        Scratch::create(&std::env::temp_dir()).unwrap()
    }

    #[test]
    fn direct_output_is_decoded_without_scratch_files() {
        let parent = parent();
        let image = read_with(&parent.0, &mut |command, limit| {
            assert_eq!(command.get_program(), "pngpaste");
            assert_eq!(limit, MAX_IMAGE_BYTES);
            Ok(output(png()))
        })
        .unwrap();
        assert_eq!(image, (vec![12, 34, 56, 255, 12, 34, 56, 255], 2, 1));
        assert_eq!(fs::read_dir(&parent.0).unwrap().count(), 0);
    }

    #[test]
    fn fallback_success_and_partial_failures_remove_private_files() {
        for fail_at in [None, Some("osascript"), Some("sips")] {
            let parent = parent();
            let image = read_with(&parent.0, &mut |command, limit| {
                let tool = command.get_program().to_str().unwrap();
                match tool {
                    "pngpaste" | "imgpaste" => return Err(io::ErrorKind::NotFound.into()),
                    "osascript" => {
                        fs::write(
                            command.get_current_dir().unwrap().join("clipboard.tiff"),
                            b"fixture",
                        )?;
                    }
                    "sips" => {
                        fs::write(command.get_args().last().unwrap(), png())?;
                    }
                    _ => unreachable!(),
                }
                assert_eq!(limit, MAX_TOOL_TEXT);
                if fail_at == Some(tool) {
                    return Err(io::ErrorKind::TimedOut.into());
                }
                Ok(output(Vec::new()))
            });
            assert_eq!(image.is_some(), fail_at.is_none());
            assert_eq!(fs::read_dir(&parent.0).unwrap().count(), 0);
        }
    }

    #[test]
    fn exhausted_capture_budget_stops_fallback_tools() {
        let parent = parent();
        let _budget = super::super::bounded_command::CaptureBudget::enter(std::sync::Arc::new(
            std::sync::atomic::AtomicBool::new(false),
        ));
        let mut calls = 0;
        assert!(read_with(&parent.0, &mut |_, _| {
            calls += 1;
            super::super::bounded_command::fail_capture();
            Err(io::ErrorKind::TimedOut.into())
        })
        .is_none());
        assert_eq!(calls, 1);
        assert_eq!(fs::read_dir(&parent.0).unwrap().count(), 0);
    }

    #[test]
    fn oversized_file_aborts_capture_before_conversion_and_cleans_up() {
        let parent = parent();
        let _budget = super::super::bounded_command::CaptureBudget::enter(std::sync::Arc::new(
            std::sync::atomic::AtomicBool::new(false),
        ));
        let image = read_with(&parent.0, &mut |command, _| {
            if command.get_program() != "osascript" {
                assert_ne!(command.get_program(), "sips");
                return Err(io::ErrorKind::NotFound.into());
            }
            fs::File::create(command.get_current_dir().unwrap().join("clipboard.tiff"))?
                .set_len(MAX_IMAGE_BYTES as u64 + 1)?;
            Ok(output(Vec::new()))
        });
        assert!(image.is_none());
        assert!(super::super::bounded_command::capture_aborted());
        assert_eq!(fs::read_dir(&parent.0).unwrap().count(), 0);
    }

    #[test]
    fn icon_conversion_preserves_target_on_partial_failure_and_publishes_success() {
        let parent = parent();
        let target = parent.0.join("clipboard.png");
        fs::write(&target, b"original icon").unwrap();
        for fail in [true, false] {
            let result =
                convert_icon_with(Path::new("fixture.icns"), &target, &mut |command, limit| {
                    assert_eq!(limit, MAX_TOOL_TEXT);
                    fs::write(command.get_args().last().unwrap(), png())?;
                    if fail {
                        return Err(io::ErrorKind::TimedOut.into());
                    }
                    Ok(output(Vec::new()))
                });
            assert_eq!(result.is_some(), !fail);
            assert_eq!(
                fs::read(&target).unwrap(),
                if fail {
                    b"original icon".to_vec()
                } else {
                    png()
                }
            );
            assert_eq!(fs::read_dir(&parent.0).unwrap().count(), 1);
        }
    }
}

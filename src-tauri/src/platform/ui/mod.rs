//! Tray, window and disk-space helpers behind one `platform::ui` module.
//!
//! | File        | Contents                                                        |
//! | ----------- | --------------------------------------------------------------- |
//! | `tray.rs`   | `SystemTray`, the recent-items submenu, and tray actions        |
//! | `window.rs` | Main-window show/restore, saved position, transparency + effects |
//! | `disk.rs`   | `DiskSpace` / `disk_space`                                     |

mod disk;
mod tray;
mod window;

pub use disk::*;
pub use tray::*;
pub use window::*;

//! Clipboard file commands, split by concern.
//!
//! | File           | Owns                                                                      |
//! | -------------- | ------------------------------------------------------------------------- |
//! | `mod.rs`       | Submodule wiring plus the re-exports that keep `commands::files::*` stable |
//! | `icons.rs`     | `IconCacheEntry`, the icon cache builder, and the list/delete commands    |
//! | `replace.rs`   | `replace_icon_file` and the source validation gate it depends on          |
//! | `clipboard.rs` | `save_clipboard_item_file` / `copy_clipboard_item_files` and path checks  |
//! | `shell.rs`     | `open_external_url` and `reveal_in_explorer` OS integrations             |
//! | `tests.rs`     | Unit tests for the icon cache, replacement, save-as, and shell commands   |

mod clipboard;
mod icons;
mod replace;
mod shell;

#[cfg(test)]
mod tests;

pub use clipboard::{copy_clipboard_item_files, save_clipboard_item_file};
pub use icons::{delete_icon_files, list_icon_cache, IconCacheEntry};
pub use replace::replace_icon_file;
pub use shell::{open_external_url, reveal_in_explorer};

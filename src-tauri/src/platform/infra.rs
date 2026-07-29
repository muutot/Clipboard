// infra module is deprecated — use autostart and single_instance modules directly.
//
// This file is kept for backward compatibility and will be removed in a future
// release.  All items are re-exported from `platform::autostart` and
// `platform::single_instance` (and re-exported through `platform::`).

#[allow(deprecated)]
pub use crate::platform::autostart::{decide_autostart_action, sync_autostart, AutostartAction};
#[allow(deprecated)]
pub use crate::platform::single_instance::{SingleInstanceError, SingleInstanceGuard};

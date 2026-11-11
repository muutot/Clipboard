mod auto_tags;
mod helpers;
mod impls;
mod snapshot;
mod summaries;
mod traits;
mod transactions;

use helpers::*;

pub use auto_tags::{AutoTagHistoryPreview, AutoTagPhase, AutoTagProgress};
pub use traits::*;
pub use transactions::TransactionalSaveSummary;
pub(crate) use transactions::validate_clipboard_text;

#[cfg(test)]
mod tests;

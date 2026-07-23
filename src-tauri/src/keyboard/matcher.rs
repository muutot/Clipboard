use std::{collections::BTreeMap, str::FromStr};

use crate::storage::StorageError;

use super::{binding::normalize_key, KeyboardConfig, Modifier, ShortcutBinding};

pub const DEFAULT_DOUBLE_TAP_INTERVAL_MS: u64 = 300;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ChordKey {
    modifiers: Vec<Modifier>,
    key: String,
}

pub struct ShortcutMatcher {
    chords: BTreeMap<ChordKey, Vec<String>>,
    double_modifiers: BTreeMap<Modifier, Vec<String>>,
    double_tap_interval_ms: u64,
    last_modifier_tap: Option<(Modifier, u64)>,
}

impl ShortcutMatcher {
    pub fn from_config(config: &KeyboardConfig) -> Result<Self, StorageError> {
        Self::with_double_tap_interval(config, DEFAULT_DOUBLE_TAP_INTERVAL_MS)
    }

    pub fn with_double_tap_interval(
        config: &KeyboardConfig,
        double_tap_interval_ms: u64,
    ) -> Result<Self, StorageError> {
        let mut matcher = Self {
            chords: BTreeMap::new(),
            double_modifiers: BTreeMap::new(),
            double_tap_interval_ms: double_tap_interval_ms.max(1),
            last_modifier_tap: None,
        };

        for (action, shortcuts) in &config.shortcuts {
            for shortcut in shortcuts {
                let binding = ShortcutBinding::from_str(shortcut)
                    .map_err(|error| StorageError::InvalidShortcut(error.to_string()))?;
                match binding {
                    ShortcutBinding::Chord { modifiers, key } => matcher
                        .chords
                        .entry(ChordKey {
                            modifiers: modifiers.into_iter().collect(),
                            key,
                        })
                        .or_default()
                        .push(action.clone()),
                    ShortcutBinding::DoubleModifier { modifier } => matcher
                        .double_modifiers
                        .entry(modifier)
                        .or_default()
                        .push(action.clone()),
                }
            }
        }

        Ok(matcher)
    }

    pub fn match_chord(
        &self,
        modifiers: impl IntoIterator<Item = Modifier>,
        key: &str,
    ) -> &[String] {
        let mut modifiers = modifiers.into_iter().collect::<Vec<_>>();
        modifiers.sort_unstable();
        modifiers.dedup();
        let chord = ChordKey {
            modifiers,
            key: normalize_key(key),
        };

        self.chords.get(&chord).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Records one completed modifier tap (press followed by release).
    /// Platform adapters should cancel the sequence if another key participates.
    pub fn record_modifier_tap(&mut self, modifier: Modifier, timestamp_ms: u64) -> &[String] {
        let matches_double_tap =
            self.last_modifier_tap
                .is_some_and(|(previous_modifier, previous_timestamp)| {
                    previous_modifier == modifier
                        && timestamp_ms >= previous_timestamp
                        && timestamp_ms - previous_timestamp <= self.double_tap_interval_ms
                });

        if matches_double_tap {
            self.last_modifier_tap = None;
            self.double_modifiers
                .get(&modifier)
                .map(Vec::as_slice)
                .unwrap_or(&[])
        } else {
            self.last_modifier_tap = Some((modifier, timestamp_ms));
            &[]
        }
    }

    pub fn cancel_modifier_sequence(&mut self) {
        self.last_modifier_tap = None;
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::ShortcutMatcher;
    use crate::keyboard::{KeyboardConfig, Modifier};

    fn config() -> KeyboardConfig {
        KeyboardConfig::from_shortcuts(BTreeMap::from([
            (
                "toggleWindow".to_owned(),
                vec!["Alt+V".to_owned(), "Shift+Shift".to_owned()],
            ),
            ("quickPaste".to_owned(), vec!["Ctrl+Enter".to_owned()]),
        ]))
    }

    #[test]
    fn one_action_can_match_multiple_binding_kinds() {
        let mut matcher = ShortcutMatcher::from_config(&config()).unwrap();

        assert_eq!(matcher.match_chord([Modifier::Alt], "v"), &["toggleWindow"]);
        assert!(matcher
            .record_modifier_tap(Modifier::Shift, 1_000)
            .is_empty());
        assert_eq!(
            matcher.record_modifier_tap(Modifier::Shift, 1_250),
            &["toggleWindow"]
        );
    }

    #[test]
    fn double_taps_expire_after_the_configured_interval() {
        let mut matcher = ShortcutMatcher::with_double_tap_interval(&config(), 200).unwrap();

        assert!(matcher
            .record_modifier_tap(Modifier::Shift, 1_000)
            .is_empty());
        assert!(matcher
            .record_modifier_tap(Modifier::Shift, 1_250)
            .is_empty());
        assert_eq!(
            matcher.record_modifier_tap(Modifier::Shift, 1_400),
            &["toggleWindow"]
        );
    }

    #[test]
    fn unrelated_input_cancels_a_pending_modifier_tap() {
        let mut matcher = ShortcutMatcher::from_config(&config()).unwrap();

        matcher.record_modifier_tap(Modifier::Shift, 1_000);
        matcher.cancel_modifier_sequence();

        assert!(matcher
            .record_modifier_tap(Modifier::Shift, 1_100)
            .is_empty());
    }
}

use std::{collections::BTreeSet, error::Error, fmt, str::FromStr};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Modifier {
    Control,
    Alt,
    Shift,
    Meta,
}

impl Modifier {
    fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "ctrl" | "control" => Some(Self::Control),
            "alt" | "option" => Some(Self::Alt),
            "shift" => Some(Self::Shift),
            "meta" | "cmd" | "command" | "super" | "win" | "windows" => Some(Self::Meta),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Control => "Ctrl",
            Self::Alt => "Alt",
            Self::Shift => "Shift",
            Self::Meta => "Meta",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShortcutBinding {
    Chord {
        modifiers: BTreeSet<Modifier>,
        key: String,
    },
    DoubleModifier {
        modifier: Modifier,
    },
}

impl ShortcutBinding {
    pub fn canonical(&self) -> String {
        match self {
            Self::Chord { modifiers, key } => modifiers
                .iter()
                .map(|modifier| modifier.label())
                .chain(std::iter::once(key.as_str()))
                .collect::<Vec<_>>()
                .join("+"),
            Self::DoubleModifier { modifier } => {
                format!("{}+{}", modifier.label(), modifier.label())
            }
        }
    }
}

impl FromStr for ShortcutBinding {
    type Err = ShortcutParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let raw = value.trim();
        let parts = raw
            .split('+')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>();

        if parts.is_empty() {
            return Err(ShortcutParseError::new(value, "shortcut cannot be empty"));
        }

        if parts.len() == 2 {
            if let (Some(first), Some(second)) =
                (Modifier::parse(parts[0]), Modifier::parse(parts[1]))
            {
                if first == second {
                    return Ok(Self::DoubleModifier { modifier: first });
                }
            }
        }

        let mut modifiers = BTreeSet::new();
        let mut key = None;

        for part in parts {
            if let Some(modifier) = Modifier::parse(part) {
                if !modifiers.insert(modifier) {
                    return Err(ShortcutParseError::new(
                        value,
                        "a modifier cannot be repeated in a chord",
                    ));
                }
                continue;
            }

            if key.is_some() {
                return Err(ShortcutParseError::new(
                    value,
                    "a chord can contain only one non-modifier key",
                ));
            }
            key = Some(normalize_key(part));
        }

        let key = key.ok_or_else(|| {
            ShortcutParseError::new(value, "a chord requires one non-modifier key")
        })?;

        Ok(Self::Chord { modifiers, key })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutParseError {
    shortcut: String,
    reason: &'static str,
}

impl ShortcutParseError {
    fn new(shortcut: &str, reason: &'static str) -> Self {
        Self {
            shortcut: shortcut.to_owned(),
            reason,
        }
    }
}

impl fmt::Display for ShortcutParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "invalid shortcut '{}': {}",
            self.shortcut, self.reason
        )
    }
}

impl Error for ShortcutParseError {}

pub(super) fn normalize_key(key: &str) -> String {
    if key.chars().count() == 1 {
        key.to_uppercase()
    } else {
        let mut characters = key.chars();
        let first = characters
            .next()
            .into_iter()
            .flat_map(char::to_uppercase)
            .collect::<String>();
        format!("{first}{}", characters.as_str().to_ascii_lowercase())
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::{Modifier, ShortcutBinding};

    #[test]
    fn normalizes_chords_to_a_stable_modifier_order() {
        let binding = ShortcutBinding::from_str("shift + ctrl + v").unwrap();

        assert_eq!(binding.canonical(), "Ctrl+Shift+V");
    }

    #[test]
    fn accepts_platform_modifier_aliases() {
        let binding = ShortcutBinding::from_str("command+space").unwrap();

        assert_eq!(binding.canonical(), "Meta+Space");
    }

    #[test]
    fn recognizes_double_modifier_shortcuts() {
        let binding = ShortcutBinding::from_str("Shift + Shift").unwrap();

        assert_eq!(
            binding,
            ShortcutBinding::DoubleModifier {
                modifier: Modifier::Shift
            }
        );
        assert_eq!(binding.canonical(), "Shift+Shift");
    }

    #[test]
    fn rejects_ambiguous_or_modifier_only_chords() {
        assert!(ShortcutBinding::from_str("Ctrl+V+X").is_err());
        assert!(ShortcutBinding::from_str("Ctrl+Alt").is_err());
        assert!(ShortcutBinding::from_str("Ctrl+Ctrl+V").is_err());
    }
}

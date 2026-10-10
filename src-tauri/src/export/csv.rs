//! CSV field escaping/parsing shared by the export and import paths.

pub(crate) const CSV_TEXT_ENCODING: &str = "apostrophe-v1";

pub(crate) fn clipboard_kind_name(kind: crate::domain::ClipboardKind) -> &'static str {
    match kind {
        crate::domain::ClipboardKind::Text => "text",
        crate::domain::ClipboardKind::Link => "link",
        crate::domain::ClipboardKind::Image => "image",
        crate::domain::ClipboardKind::File => "file",
    }
}

pub(crate) fn escape_csv(field: &str) -> String {
    let starts_as_formula = field
        .trim_start()
        .chars()
        .next()
        .is_some_and(|c| matches!(c, '=' | '+' | '-' | '@'));
    // CSV quotes delimit a cell; they do not make its contents literal in a
    // spreadsheet. Prefix formula-looking text, and escape an original leading
    // apostrophe too so our versioned importer can reverse this exactly.
    let literal = if starts_as_formula || field.starts_with('\'') {
        std::borrow::Cow::Owned(format!("'{field}"))
    } else {
        std::borrow::Cow::Borrowed(field)
    };
    let field = literal.as_ref();
    if field.contains(',')
        || field.contains('"')
        || field.contains('\n')
        || field.contains('\r')
        || field.starts_with('\'')
    {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_owned()
    }
}

/// Minimal RFC 4180-style CSV reader matching the quoting rules used by
/// `escape_csv`. Operates on `char`s so multi-byte UTF-8 content (Chinese
/// titles, emoji, etc.) is preserved without splitting code points.
pub(crate) struct CsvReader {
    chars: Vec<char>,
    pos: usize,
}

impl CsvReader {
    pub(super) fn new(input: &str) -> Self {
        Self {
            chars: input.chars().collect(),
            pos: 0,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek_next(&self) -> Option<char> {
        self.chars.get(self.pos + 1).copied()
    }

    /// Returns the next record as a list of (unquoted) field values, or `None`
    /// at end of input.
    pub(super) fn next_record(&mut self) -> Option<Vec<String>> {
        if self.pos >= self.chars.len() {
            return None;
        }

        let mut fields = Vec::new();
        loop {
            fields.push(self.read_field());
            match self.peek() {
                Some(',') => self.pos += 1,
                Some('\n') => {
                    self.pos += 1;
                    break;
                }
                Some('\r') => {
                    self.pos += 1;
                    if self.peek() == Some('\n') {
                        self.pos += 1;
                    }
                    break;
                }
                _ => break,
            }
        }
        Some(fields)
    }

    fn read_field(&mut self) -> String {
        if self.peek() == Some('"') {
            self.pos += 1;
            let mut value = String::new();
            while let Some(character) = self.peek() {
                if character == '"' {
                    if self.peek_next() == Some('"') {
                        value.push('"');
                        self.pos += 2;
                    } else {
                        self.pos += 1;
                        break;
                    }
                } else {
                    value.push(character);
                    self.pos += 1;
                }
            }
            value
        } else {
            let mut value = String::new();
            while let Some(character) = self.peek() {
                if matches!(character, ',' | '\r' | '\n') {
                    break;
                }
                value.push(character);
                self.pos += 1;
            }
            value
        }
    }
}

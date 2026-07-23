use std::{error::Error, fmt};

#[derive(Debug)]
pub enum SearchError {
    Io(std::io::Error),
    Tantivy(tantivy::TantivyError),
    WriterPoisoned,
    MissingStoredField(&'static str),
}

impl fmt::Display for SearchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "search index I/O error: {error}"),
            Self::Tantivy(error) => write!(formatter, "Tantivy search error: {error}"),
            Self::WriterPoisoned => formatter.write_str("search index writer lock is poisoned"),
            Self::MissingStoredField(field) => {
                write!(formatter, "search result is missing stored field: {field}")
            }
        }
    }
}

impl Error for SearchError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Tantivy(error) => Some(error),
            _ => None,
        }
    }
}

impl From<std::io::Error> for SearchError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<tantivy::TantivyError> for SearchError {
    fn from(error: tantivy::TantivyError) -> Self {
        Self::Tantivy(error)
    }
}

use std::fmt;

#[derive(Debug)]
pub enum GqlError {
    /// The server doesn't know the hash, or the manifest doesn't have the operation
    UnknownHash {
        op: &'static str,
    },
    Graphql {
        code: Option<String>,
        message: String,
    },
    Http {
        status: u16,
    },
    Network(reqwest::Error),
    Parse(serde_json::Error),
    Manifest(String),
}

impl fmt::Display for GqlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownHash { op } => write!(f, "unknown persisted query hash for {op}"),
            Self::Graphql { code, message } => {
                write!(
                    f,
                    "GraphQL error {}: {message}",
                    code.as_deref().unwrap_or("-")
                )
            }
            Self::Http { status } => write!(f, "HTTP {status}"),
            Self::Network(e) => write!(f, "network error: {e}"),
            Self::Parse(e) => write!(f, "can't parse the answer: {e}"),
            Self::Manifest(m) => write!(f, "manifest: {m}"),
        }
    }
}

impl std::error::Error for GqlError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Network(e) => Some(e),
            Self::Parse(e) => Some(e),
            _ => None,
        }
    }
}

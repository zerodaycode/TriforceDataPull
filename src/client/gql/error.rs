use std::{error::Error, fmt};

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
    Body(crate::client::BodyError),
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
            Self::Body(e) => write!(f, "{e}"),
            Self::Parse(e) => write!(f, "can't parse the answer: {e}"),
            Self::Manifest(m) => write!(f, "manifest: {m}"),
        }
    }
}

impl Error for GqlError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            // Display already has the inner error: start the chain one level down
            Self::Network(e) => e.source(),
            Self::Body(e) => e.source(),
            Self::Parse(e) => e.source(),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chain_has_each_message_once() {
        let inner = serde_json::from_str::<u8>("x").unwrap_err();
        let text = inner.to_string();
        let e = color_eyre::Report::new(GqlError::Parse(inner));
        assert_eq!(format!("{e:#}").matches(&text).count(), 1);
    }
}

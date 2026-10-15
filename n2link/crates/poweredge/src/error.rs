use thiserror::Error;

/// Every rejection carries a stable machine code so the guided app can map it to a
/// plain-language message and a next step.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum Error {
    #[error("intent rejected: {0}")]
    Intent(String),
    #[error("template not found: {id}@{version}")]
    UnknownTemplate { id: String, version: String },
    #[error("template integrity failure: {0}")]
    Integrity(String),
    #[error("parameter `{slot}` rejected: {reason}")]
    Parameter { slot: String, reason: String },
    #[error("environment `{0}` is not allowed by this template")]
    Environment(String),
    #[error("context value `{0}` is missing")]
    Context(String),
    #[error("graph rejected: {0}")]
    Graph(String),
    #[error("io: {0}")]
    Io(String),
}

impl Error {
    pub fn code(&self) -> &'static str {
        match self {
            Error::Intent(_) => "invalid_intent",
            Error::UnknownTemplate { .. } => "unknown_template",
            Error::Integrity(_) => "template_integrity",
            Error::Parameter { .. } => "invalid_parameter",
            Error::Environment(_) => "environment_not_allowed",
            Error::Context(_) => "missing_context",
            Error::Graph(_) => "unsafe_graph",
            Error::Io(_) => "io",
        }
    }

    pub(crate) fn param(slot: &str, reason: impl Into<String>) -> Self {
        Error::Parameter { slot: slot.to_owned(), reason: reason.into() }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

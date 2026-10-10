//! Error type and process exit-code mapping.

use std::fmt;

/// A CLI error that knows the process exit code it should produce.
#[derive(Debug)]
pub enum Error {
    /// Bad arguments or an invalid record: exit status 2.
    Usage(String),
    /// A runtime or database failure: exit status 1.
    Runtime(String),
}

impl Error {
    pub fn usage(msg: impl Into<String>) -> Self {
        Error::Usage(msg.into())
    }

    pub fn runtime(msg: impl Into<String>) -> Self {
        Error::Runtime(msg.into())
    }

    /// Process exit status: 0 success, 1 runtime, 2 usage.
    pub fn exit_code(&self) -> u8 {
        match self {
            Error::Usage(_) => 2,
            Error::Runtime(_) => 1,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Usage(m) | Error::Runtime(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<honker::Error> for Error {
    fn from(e: honker::Error) -> Self {
        Error::Runtime(e.to_string())
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Runtime(e.to_string())
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Runtime(e.to_string())
    }
}

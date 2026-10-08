//! The single error type. Every failure reaches the user as a structured error
//! with a stable code, a message, and optional suggestions and details.

use std::fmt;
use std::ops::{Deref, DerefMut};

use serde_json::{Map, Value, json};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Code {
    Validation,
    Config,
    ConfigInsecure,
    Protocol,
    UnsafeRedirect,
    Api,
    BadRequest,
    Auth,
    Forbidden,
    NotFound,
    Conflict,
    RateLimited,
    Timeout,
    Network,
}

impl Code {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Validation => "VALIDATION_ERROR",
            Self::Config => "CONFIG_ERROR",
            Self::ConfigInsecure => "CONFIG_INSECURE",
            Self::Protocol => "PROTOCOL_ERROR",
            Self::UnsafeRedirect => "UNSAFE_REDIRECT",
            Self::Api => "API_ERROR",
            Self::BadRequest => "BAD_REQUEST",
            Self::Auth => "AUTH_ERROR",
            Self::Forbidden => "FORBIDDEN",
            Self::NotFound => "NOT_FOUND",
            Self::Conflict => "CONFLICT",
            Self::RateLimited => "RATE_LIMITED",
            Self::Timeout => "TIMEOUT",
            Self::Network => "NETWORK_ERROR",
        }
    }
}

/// What an error says. Boxed inside [`Error`] so `Result` stays small.
#[derive(Debug)]
pub struct Details {
    pub code: Code,
    pub message: String,
    pub suggestions: Vec<String>,
    pub details: Option<Value>,
}

#[derive(Debug)]
pub struct Error(Box<Details>);

impl Deref for Error {
    type Target = Details;

    fn deref(&self) -> &Details {
        &self.0
    }
}

impl DerefMut for Error {
    fn deref_mut(&mut self) -> &mut Details {
        &mut self.0
    }
}

impl Error {
    pub fn new(code: Code, message: impl Into<String>) -> Self {
        Self(Box::new(Details {
            code,
            message: message.into(),
            suggestions: Vec::new(),
            details: None,
        }))
    }

    pub fn validation(message: impl Into<String>) -> Self {
        Self::new(Code::Validation, message)
    }

    pub fn config(message: impl Into<String>, path: &std::path::Path) -> Self {
        Self::new(Code::Config, message).with_details(json!({ "path": path.display().to_string() }))
    }

    pub fn insecure(message: impl Into<String>, path: &std::path::Path) -> Self {
        Self::new(Code::ConfigInsecure, message)
            .with_suggestion("Use a regular file owned and readable only by your user (mode 0600).")
            .with_details(json!({ "path": path.display().to_string() }))
    }

    /// Hevy sent something the CLI cannot interpret.
    pub fn protocol(message: impl Into<String>) -> Self {
        Self::new(Code::Protocol, message)
    }

    pub fn with_suggestion(mut self, suggestion: impl Into<String>) -> Self {
        self.0.suggestions.push(suggestion.into());
        self
    }

    pub fn with_details(mut self, details: Value) -> Self {
        self.0.details = Some(details);
        self
    }

    /// Usage mistakes and unsafe local configuration exit 2; everything else 1.
    pub fn exit_code(&self) -> u8 {
        match self.code {
            Code::Validation | Code::ConfigInsecure => 2,
            _ => 1,
        }
    }

    pub fn to_value(&self) -> Value {
        let mut error = Map::new();
        error.insert("code".into(), self.code.as_str().into());
        error.insert("message".into(), self.message.clone().into());
        if let Some(details) = &self.details {
            error.insert("details".into(), details.clone());
        }
        error.insert("suggestions".into(), json!(self.suggestions));
        json!({ "error": error })
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for Error {}

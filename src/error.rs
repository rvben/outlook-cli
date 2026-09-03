use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("{0}")]
    InvalidInput(String),
    #[error("{0}")]
    Auth(String),
    #[error("{0}")]
    ReadOnly(String),
    #[error("{0}")]
    Permission(String),
    #[error("{0}")]
    NotFound(String),
    #[error("Microsoft Graph throttled the request")]
    RateLimit(Option<u64>),
    #[error("{0}")]
    Api(String),
    #[error("{0}")]
    NonInteractive(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Unexpected(String),
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct ErrorContract {
    pub kind: &'static str,
    pub exit_code: i32,
    pub retryable: bool,
    pub description: &'static str,
}

pub const ALL: &[ErrorContract] = &[
    ErrorContract {
        kind: "invalid_input",
        exit_code: 2,
        retryable: false,
        description: "Arguments or local configuration are invalid",
    },
    ErrorContract {
        kind: "auth",
        exit_code: 3,
        retryable: false,
        description: "Authentication is missing, expired, or rejected",
    },
    ErrorContract {
        kind: "read_only",
        exit_code: 2,
        retryable: false,
        description: "The active profile blocks remote write operations",
    },
    ErrorContract {
        kind: "not_found",
        exit_code: 4,
        retryable: false,
        description: "The requested Outlook resource does not exist",
    },
    ErrorContract {
        kind: "permission_denied",
        exit_code: 5,
        retryable: false,
        description: "The tenant or granted scopes do not permit the operation",
    },
    ErrorContract {
        kind: "api_error",
        exit_code: 5,
        retryable: false,
        description: "Microsoft Graph returned an API error",
    },
    ErrorContract {
        kind: "rate_limit",
        exit_code: 6,
        retryable: true,
        description: "Microsoft Graph throttled the request",
    },
    ErrorContract {
        kind: "tty_required",
        exit_code: 2,
        retryable: false,
        description: "An interactive command was invoked without a terminal",
    },
    ErrorContract {
        kind: "unexpected_error",
        exit_code: 1,
        retryable: false,
        description: "An unexpected local or transport error occurred",
    },
];

impl AppError {
    pub fn contract(&self) -> ErrorContract {
        let kind = match self {
            Self::InvalidInput(_) => "invalid_input",
            Self::Auth(_) => "auth",
            Self::ReadOnly(_) => "read_only",
            Self::NotFound(_) => "not_found",
            Self::Permission(_) => "permission_denied",
            Self::Api(_) => "api_error",
            Self::RateLimit(_) => "rate_limit",
            Self::NonInteractive(_) => "tty_required",
            Self::Io(_) | Self::Unexpected(_) => "unexpected_error",
        };
        *ALL.iter().find(|contract| contract.kind == kind).unwrap()
    }
}

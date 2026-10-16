use thiserror::Error;

use crate::dates::DateError;

/// LLM 向けエラー（spec 6.2.3 / design「errors.rs」）。
///
/// `Display` がそのまま LLM に返る文言になる。token は含めない。
#[derive(Debug, Error)]
pub enum ToolError {
    #[error("Invalid date. Please re-specify as a concrete date in YYYY-MM-DD format: {input}")]
    InvalidDate { input: String },

    #[error("Parameter error: {message}")]
    InvalidParams { message: String },

    #[error("User not found: {handle}")]
    NoSuchUser { handle: String },

    #[error("Could not resolve remote user: {handle}")]
    FailedToResolveRemoteUser { handle: String },

    #[error("Full-text search is unavailable (UNAVAILABLE); use the users/notes path")]
    SearchUnavailable,

    #[error("Rate limit reached. Please retry after a while")]
    RateLimited,

    #[error("Misskey API error ({code}): {message}")]
    Api { code: String, message: String },

    #[error("HTTP error: {message}")]
    Http { message: String },
}

impl From<DateError> for ToolError {
    fn from(err: DateError) -> Self {
        Self::InvalidDate { input: err.input }
    }
}

impl ToolError {
    /// Misskey API のエラーコードを LLM 向けメッセージに変換する（spec 5.2 / 6.2.4）。
    pub fn api(status: u16, code: &str, message: &str, handle: &str) -> Self {
        match code {
            "NO_SUCH_USER" => Self::NoSuchUser {
                handle: handle.to_string(),
            },
            "FAILED_TO_RESOLVE_REMOTE_USER" => Self::FailedToResolveRemoteUser {
                handle: handle.to_string(),
            },
            "UNAVAILABLE" => Self::SearchUnavailable,
            "RATE_LIMIT_EXCEEDED" => Self::RateLimited,
            _ => Self::Api {
                code: if code.is_empty() {
                    status.to_string()
                } else {
                    code.to_string()
                },
                message: message.to_string(),
            },
        }
    }
}

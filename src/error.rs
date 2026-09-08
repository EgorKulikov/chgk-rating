use thiserror::Error;

/// Errors from both the public API and the write-side site flows.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    /// The site answered a write request with its login page: the stored
    /// session cookies are no longer accepted (or the client was never
    /// logged in). Ask the user to log in again, then retry the action.
    #[error("rating.chgk.info session expired or not logged in")]
    SessionExpired,

    /// [`login`](crate::site::login) did not produce a logged-in session:
    /// wrong credentials, a captcha, or a changed login form. The message
    /// carries the site's own error text when it showed one.
    #[error("rating.chgk.info login failed: {0}")]
    LoginFailed(String),

    /// Transport-level failure, timeout, or an unparsable JSON body.
    #[error("rating.chgk.info request failed: {0}")]
    Http(#[from] reqwest::Error),

    /// Non-2xx status from `url`; `summary` is a short human-readable
    /// extract of the response body.
    #[error("{url} returned HTTP {status}: {summary}")]
    Status {
        /// HTTP status code.
        status: u16,
        /// The request URL (path and query; no credentials).
        url: String,
        /// Visible text of the response, capped at 300 characters.
        summary: String,
    },

    /// The site accepted the request but reported failure in the body, or
    /// signalled it by silently doing nothing. `action` names the call
    /// (`create_team`, `upload_rosters`, ...).
    #[error("rating.chgk.info {action}: {message}")]
    Site {
        /// The client call that failed.
        action: String,
        /// The site's message, or a description of what it did instead.
        message: String,
    },
    /// A response or a stored value that did not have the expected shape:
    /// undecodable JSON (the message names the endpoint), a body over
    /// [`MAX_RESPONSE_BYTES`](crate::MAX_RESPONSE_BYTES), a paginated
    /// collection larger than the client is willing to walk, unreadable
    /// persisted cookies.
    #[error("could not parse rating.chgk.info response: {0}")]
    Parse(String),

    /// A caller-supplied setting is invalid (a base URL that does not
    /// parse, a cookie name or value with forbidden characters).
    #[error("invalid configuration: {0}")]
    Config(String),
}

/// `Result` with this crate's [`enum@Error`].
pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    /// True for [`Error::SessionExpired`] — the one case where the right
    /// reaction is "log in again and retry" rather than "report failure".
    pub fn is_session_expired(&self) -> bool {
        matches!(self, Error::SessionExpired)
    }

    /// The HTTP status code for [`Error::Status`], `None` otherwise.
    pub fn status(&self) -> Option<u16> {
        match self {
            Error::Status { status, .. } => Some(*status),
            _ => None,
        }
    }

    pub(crate) fn site(action: &str, message: impl Into<String>) -> Error {
        Error::Site {
            action: action.to_string(),
            message: message.into(),
        }
    }
}

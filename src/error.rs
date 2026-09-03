use thiserror::Error;

/// Errors from both the public API and the write-side site flows.
#[derive(Debug, Error)]
pub enum Error {
    /// The site answered a write request with its login page: the stored
    /// session cookies are no longer accepted (or the client was never
    /// logged in). Ask the user to log in again, then retry the action.
    #[error("rating.chgk.info session expired or not logged in")]
    SessionExpired,

    /// `login` did not produce a logged-in session (wrong credentials,
    /// captcha, changed login form...).
    #[error("rating.chgk.info login failed: {0}")]
    LoginFailed(String),

    /// Transport-level failure or an unparsable JSON body.
    #[error("rating.chgk.info request failed: {0}")]
    Http(#[from] reqwest::Error),

    /// Non-2xx/3xx status; `summary` is a short human-readable extract of
    /// the response body.
    #[error("rating.chgk.info returned HTTP {status}: {summary}")]
    Status { status: u16, summary: String },

    /// The site accepted the request but reported failure in the body, or
    /// signalled it by silently doing nothing.
    #[error("rating.chgk.info: {0}")]
    Site(String),

    /// A response that did not have the expected shape.
    #[error("could not parse rating.chgk.info response: {0}")]
    Parse(String),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    /// True for [`Error::SessionExpired`] — the one case where the right
    /// reaction is "log in again and retry" rather than "report failure".
    pub fn is_session_expired(&self) -> bool {
        matches!(self, Error::SessionExpired)
    }
}

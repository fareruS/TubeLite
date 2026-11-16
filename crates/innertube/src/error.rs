use thiserror::Error;

/// Errors from the InnerTube transport. No panics on malformed
/// responses: a bad body is a [`Error::Json`], a bad status is a
/// [`Error::HttpStatus`].
#[derive(Debug, Error)]
pub enum Error {
    /// A transport failure from reqwest (connect, timeout, TLS, ...).
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    /// The response body was not valid JSON, or a JSON value had an
    /// unexpected shape where a shape was required.
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    /// The session is dead: a 401/403 arrived while signed in and the
    /// heal (if any) did not produce a fresh cookie. The caller should
    /// ask the user to sign in again.
    #[error("session expired")]
    SessionExpired,

    /// No client with that name exists in the loaded `clients.json`.
    #[error("unknown client: {0}")]
    UnknownClient(String),

    /// A non-2xx HTTP status that is not a healable 401/403.
    #[error("unexpected http status: {0}")]
    HttpStatus(u16),

    /// A response or file had a shape this crate cannot use.
    #[error("invalid response: {0}")]
    InvalidResponse(String),

    /// A header value could not be built (e.g. a cookie with control
    /// characters in it).
    #[error("header error: {0}")]
    Header(#[from] reqwest::header::InvalidHeaderValue),

    /// Reading or writing a file (clients override, cookie file).
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

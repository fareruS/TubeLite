use thiserror::Error;

/// Failures are classified so callers can retry correctly: an expired URL
/// is re-resolved with the next fallback client; a 403 is not misreported
/// as "unsupported format"; a network error is retried boundedly.
#[derive(Debug, Error)]
pub enum SourceError {
    #[error("video not found: {0}")]
    NotFound(String),
    #[error("stream URL expired — re-resolve required")]
    UrlExpired,
    #[error("forbidden (403) — try next client in the fallback chain")]
    Forbidden,
    #[error("cipher / signature transform failed")]
    Cipher,
    #[error("proof-of-origin token unavailable")]
    PoToken,
    #[error("network error: {0}")]
    Network(String),
    #[error("upstream returned an unexpected shape: {0}")]
    UnexpectedShape(String),
    #[error("fixture / parse error: {0}")]
    Parse(String),
    #[error("sidecar error: {0}")]
    Sidecar(String),
    #[error("not supported by this source mode: {0}")]
    Unsupported(String),
}

pub type Result<T> = std::result::Result<T, SourceError>;

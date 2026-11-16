use thiserror::Error;

/// Errors from the feeds crate.
#[derive(Debug, Error)]
pub enum Error {
    /// An error from the InnerTube transport.
    #[error("innertube error: {0}")]
    Innertube(#[from] innertube::Error),

    /// An HTTP error from the RSS fallback fetch.
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    /// The RSS XML could not be parsed.
    #[error("xml error: {0}")]
    Xml(#[from] quick_xml::Error),

    /// The function has no captured fixture to build on. The message
    /// names the capture that would unblock it.
    #[error("missing fixture: {0}")]
    MissingFixture(String),

    /// The response was not the shape the function expects (for
    /// example `watch_next` needs a two-item JSON array).
    #[error("unexpected response shape: {0}")]
    UnexpectedShape(String),
}

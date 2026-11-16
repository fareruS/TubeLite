use thiserror::Error;

/// Errors from the accounts crate.
///
/// No variant carries a cookie value. In particular the keyring
/// mapping (`from_keyring`) never formats the underlying keyring
/// error: `BadEncoding` / `BadDataFormat` carry raw secret bytes, so
/// their content is replaced with a fixed message.
#[derive(Debug, Error)]
pub enum Error {
    #[error("account storage failed: {0}")]
    Storage(#[from] rusqlite::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// A cookie import with no youtube.com or google.com cookies in it.
    #[error("no youtube.com or google.com cookies found")]
    NoCookies,

    /// The cookies carry no SAPISID (or `__Secure-3PAPISID`) — the
    /// value SAPISIDHASH is computed from — so there is no account
    /// to store.
    #[error("no SAPISID in the cookies")]
    NotSignedIn,

    /// The user closed the sign-in window before finishing.
    #[error("sign-in window closed")]
    LoginCancelled,

    /// The sign-in window stayed open past the timeout.
    #[error("sign-in timed out")]
    LoginTimeout,

    /// The login flow itself failed (window could not open, cookie
    /// store unreadable, ...).
    #[error("login flow failed: {0}")]
    Flow(String),

    /// No account with this id exists.
    #[error("unknown account: {0}")]
    UnknownAccount(String),

    /// The OS keyring (or the encrypted-file fallback) failed. The
    /// message is a fixed string per failure kind — never the
    /// underlying error, which can carry secret bytes.
    #[error("secret storage failed: {0}")]
    Keyring(&'static str),

    /// The encrypted-file fallback could not decrypt a stored secret
    /// (wrong key or corrupt file).
    #[error("stored secret is unreadable: {0}")]
    Crypto(&'static str),

    /// An operation whose request/response shape is not in the
    /// fixtures yet. The message names the capture that is missing
    /// (see `docs/missing-captures.md` §1).
    #[error("not implemented (missing capture): {0}")]
    MissingCapture(&'static str),
}

impl Error {
    /// Map a keyring error to a fixed, redacted message. The
    /// keyring error's own text is never used: `BadEncoding` and
    /// `BadDataFormat` carry the raw secret bytes.
    pub(crate) fn from_keyring(error: keyring::Error) -> Self {
        use keyring::Error as K;
        match error {
            K::NoEntry => Self::Keyring("no such entry"),
            K::PlatformFailure(_) => Self::Keyring("platform keyring failure"),
            K::NoStorageAccess(_) => Self::Keyring("credential store locked or inaccessible"),
            K::BadEncoding(_) => Self::Keyring("stored secret is not valid UTF-8"),
            K::BadDataFormat(..) => Self::Keyring("stored secret is corrupt"),
            K::BadStoreFormat(_) => Self::Keyring("credential store is corrupt"),
            K::TooLong(..) => Self::Keyring("entry name exceeds the platform limit"),
            K::Invalid(..) => Self::Keyring("invalid keyring parameter"),
            K::Ambiguous(_) => Self::Keyring("multiple matching keyring entries"),
            K::NoDefaultStore => Self::Keyring("no OS keyring available"),
            K::NotSupportedByStore(_) => Self::Keyring("operation not supported by the keyring"),
            // `keyring::Error` is non-exhaustive: future variants map
            // here, still without their text.
            _ => Self::Keyring("keyring operation failed"),
        }
    }
}

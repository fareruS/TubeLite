//! [`SecretString`]: a string that never prints its content.
//!
//! Cookie values are secrets. They must not reach tracing output,
//! error messages, test output, or `Debug`/`Display` of any struct
//! that holds one. `SecretString` enforces that at the type level:
//! its `Debug` and `Display` impls print only `[redacted]`, and the
//! value comes out only through [`SecretString::expose`], which the
//! caller must invoke deliberately at the exact place the value is
//! needed (keyring write, session build, header send).
//!
//! Rules for this crate:
//! - Never `derive(Debug)` on a struct holding a `SecretString`
//!   unless the field is `SecretString` itself (its `Debug` is safe).
//! - Never put a `SecretString` (or anything derived from it) in an
//!   error message, a tracing field, or a test assertion message.
//! - `PartialEq` compares values, but never prints them.

/// A secret string. `Debug` and `Display` both print `[redacted]`.
#[derive(Clone, PartialEq, Eq)]
pub struct SecretString(String);

impl SecretString {
    /// Wrap a secret value.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The secret value. Call only where the value is actually
    /// needed. Never log, print, or include the result in an error.
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// Whether the secret is empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Length in bytes, for diagnostics that must not show content.
    pub fn len(&self) -> usize {
        self.0.len()
    }
}

impl std::fmt::Debug for SecretString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[redacted]")
    }
}

impl std::fmt::Display for SecretString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[redacted]")
    }
}

impl From<String> for SecretString {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl From<&str> for SecretString {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_and_display_never_print_the_content() {
        let secret = SecretString::new("SAPISID=do-not-print-me");
        assert_eq!(format!("{secret:?}"), "[redacted]");
        assert_eq!(format!("{secret}"), "[redacted]");
        // Even inside another struct's derived Debug, only the
        // redaction shows.
        #[derive(Debug)]
        struct Holder {
            #[allow(dead_code)]
            secret: SecretString,
        }
        let shown = format!("{:?}", Holder { secret });
        assert!(!shown.contains("do-not-print-me"));
        assert!(shown.contains("[redacted]"));
    }

    #[test]
    fn expose_returns_the_value_for_internal_use() {
        let secret = SecretString::new("abc");
        assert_eq!(secret.expose(), "abc");
        assert!(!secret.is_empty());
        assert_eq!(secret.len(), 3);
    }
}

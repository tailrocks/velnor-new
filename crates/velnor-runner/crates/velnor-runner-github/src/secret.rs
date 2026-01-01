//! JIT material. `Debug` is redacted. There is no `Display`.

use std::fmt;

use zeroize::Zeroize;

/// One-shot encoded JIT configuration.
pub struct EncodedJit {
    raw: String,
}

impl EncodedJit {
    /// Wrap a service payload. The caller must not log `raw`.
    #[must_use]
    pub fn new(raw: String) -> Self {
        Self { raw }
    }

    /// Borrow the payload for the stdin writer only.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.raw
    }
}

impl fmt::Debug for EncodedJit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("EncodedJit([redacted])")
    }
}

impl Drop for EncodedJit {
    fn drop(&mut self) {
        self.raw.zeroize();
    }
}

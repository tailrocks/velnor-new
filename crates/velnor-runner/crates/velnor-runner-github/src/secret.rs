use std::fmt;

use zeroize::Zeroize;

pub struct EncodedJit {
    raw: String,
}

impl EncodedJit {
    #[must_use]
    pub fn new(raw: String) -> Self {
        Self { raw }
    }

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

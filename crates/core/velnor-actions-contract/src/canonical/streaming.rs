//! Incremental BLAKE3 digest construction using the canonical digest format.

/// Incremental builder for the canonical `b3-<hex>` digest format.
#[derive(Debug)]
pub struct Blake3Accumulator(blake3::Hasher);

impl Blake3Accumulator {
    /// Start an empty digest.
    #[must_use]
    pub fn new() -> Self {
        Self(blake3::Hasher::new())
    }

    /// Add one byte chunk in order.
    pub fn update(&mut self, chunk: &[u8]) {
        self.0.update(chunk);
    }

    /// Finish with the same representation as [`super::digest_b3`].
    #[must_use]
    pub fn finalize(self) -> String {
        format!("b3-{}", self.0.finalize().to_hex())
    }
}

impl Default for Blake3Accumulator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::Blake3Accumulator;

    #[test]
    fn streaming_chunks_match_canonical_byte_digest() {
        let bytes = b"one file split over several reads";
        let mut digest = Blake3Accumulator::new();
        digest.update(&bytes[..3]);
        digest.update(&bytes[3..17]);
        digest.update(&bytes[17..]);
        assert_eq!(digest.finalize(), super::super::digest_b3(bytes));
    }
}

//! Synchronous registry fixture. It links `itoa` and never launches a process.

#![forbid(unsafe_code)]

/// Consume the genuine registry dependency without launching any process.
#[must_use]
pub fn decimal_length(value: u64) -> usize {
    let mut buffer = itoa::Buffer::new();
    buffer.format(value).len()
}

#[cfg(test)]
mod tests {
    #[test]
    fn registry_formatting_is_used() {
        assert_eq!(super::decimal_length(123_456_789), 9);
    }
}

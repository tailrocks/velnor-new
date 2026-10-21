//! Encode digest bytes as lowercase hexadecimal.

pub(crate) fn lowercase_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::lowercase_hex;

    #[test]
    fn lowercase_hex_encodes_every_byte_and_nibble() {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let bytes: Vec<u8> = (0..=u8::MAX).collect();
        let encoded = lowercase_hex(&bytes);

        assert_eq!(encoded.len(), bytes.len() * 2);
        for (value, pair) in bytes.iter().zip(encoded.as_bytes().chunks_exact(2)) {
            assert_eq!(pair[0], HEX[usize::from(value >> 4)]);
            assert_eq!(pair[1], HEX[usize::from(value & 0x0f)]);
        }
    }
}

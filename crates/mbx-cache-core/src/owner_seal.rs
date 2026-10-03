//! Local owner capability integrity using the existing vetted BLAKE3 primitive.

/// Seal local capability bytes with a private 256-bit owner key.
///
/// This establishes local consistency only. It does not authenticate transported
/// source provenance, and cannot protect a key from its own operating-system user.
pub fn local_owner_seal(key: &[u8; 32], bytes: &[u8]) -> [u8; 32] {
    *blake3::keyed_hash(key, bytes).as_bytes()
}

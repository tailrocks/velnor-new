use super::StageError;

/// Carries untrusted inode and content observations between workflow steps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageReceiptClaim {
    device: u64,
    inode: u64,
    length: u64,
    digest: [u8; 32],
}

impl StageReceiptClaim {
    pub(crate) fn new(device: u64, inode: u64, length: u64, digest: [u8; 32]) -> Self {
        Self {
            device,
            inode,
            length,
            digest,
        }
    }

    /// Parses the canonical compact claim. This does not verify filesystem custody.
    ///
    /// # Errors
    ///
    /// Returns an error when the claim is malformed or uses a noncanonical encoding.
    pub fn parse(text: &str) -> Result<Self, StageError> {
        let mut parts = text.split('.');
        if parts.next() != Some("v1") {
            return Err("cache_stage_claim_invalid");
        }
        let device = parse_hex_u64(parts.next().ok_or("cache_stage_claim_invalid")?)?;
        let inode = parse_hex_u64(parts.next().ok_or("cache_stage_claim_invalid")?)?;
        let length = parse_decimal(parts.next().ok_or("cache_stage_claim_invalid")?)?;
        let digest_text = parts.next().ok_or("cache_stage_claim_invalid")?;
        if parts.next().is_some() || digest_text.len() != 64 {
            return Err("cache_stage_claim_invalid");
        }
        let mut digest = [0_u8; 32];
        let (pairs, _) = digest_text.as_bytes().as_chunks::<2>();
        for (index, pair) in pairs.iter().enumerate() {
            digest[index] = (hex_nibble(pair[0])? << 4) | hex_nibble(pair[1])?;
        }
        let claim = Self {
            device,
            inode,
            length,
            digest,
        };
        if claim.encode() != text {
            return Err("cache_stage_claim_invalid");
        }
        Ok(claim)
    }

    /// Encodes the claim without adding path or cache context.
    #[must_use]
    pub fn encode(&self) -> String {
        format!(
            "v1.{:x}.{:x}.{}.{}",
            self.device,
            self.inode,
            self.length,
            hex_bytes(&self.digest)
        )
    }

    pub(crate) fn device(&self) -> u64 {
        self.device
    }

    pub(crate) fn inode(&self) -> u64 {
        self.inode
    }

    pub(crate) fn length(&self) -> u64 {
        self.length
    }

    pub(crate) fn digest(&self) -> &[u8; 32] {
        &self.digest
    }
}

fn parse_hex_u64(text: &str) -> Result<u64, StageError> {
    if text.is_empty()
        || text.len() > 16
        || text
            .bytes()
            .any(|byte| !byte.is_ascii_digit() && !(b'a'..=b'f').contains(&byte))
        || (text.len() > 1 && text.starts_with('0'))
    {
        return Err("cache_stage_claim_invalid");
    }
    u64::from_str_radix(text, 16).map_err(|_| "cache_stage_claim_invalid")
}

fn parse_decimal(text: &str) -> Result<u64, StageError> {
    if text.is_empty()
        || !text.bytes().all(|byte| byte.is_ascii_digit())
        || (text.len() > 1 && text.starts_with('0'))
    {
        return Err("cache_stage_claim_invalid");
    }
    text.parse().map_err(|_| "cache_stage_claim_invalid")
}

fn hex_nibble(byte: u8) -> Result<u8, StageError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err("cache_stage_claim_invalid"),
    }
}

fn hex_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

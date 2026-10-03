//! Fixed Public Good trust snapshot; caller/cache JSON never supplies authority.
use velnor_actions_contract::compiled_source_sha256;

const RAW_SHA256: &str = "6494e21ea73fa7ee769f85f57d5a3e6a08725eae1e38c755fc3517c9e6bc0b66";
const VERIFIER_SHA256: &str = "3c2cc7f357dc064ec527fdcd78da6e9245c21a381e1abaa0f2b62b186bcac1a1";
const QUALIFICATION_SHA256: &str =
    "2e1e1cf4777b23dd0eaa755508165d7a12d49dac0ef71b9179a62c45c0b144d9";
const RAW_BYTES: &[u8] = include_bytes!("cache_receipt_public_good/trusted_root.json");
const VERIFIER_BYTES: &[u8] = include_bytes!("cache_receipt_public_good/trusted_root.jsonl");
const QUALIFICATION_BYTES: &[u8] = include_bytes!("cache_receipt_public_good/qualification.json");

// Hashes bind retained signed TUF metadata and actual verifier output. Signature
// verification happened in the recorded pinned GH qualification, not here.
const EVIDENCE: &[(&[u8], &str)] = &[
    (RAW_BYTES, RAW_SHA256),
    (VERIFIER_BYTES, VERIFIER_SHA256),
    (QUALIFICATION_BYTES, QUALIFICATION_SHA256),
    (
        include_bytes!("cache_receipt_public_good/bootstrap_root.json"),
        "a0dfcc5d51c1ce4a66b541a3fff0afa97225ccc40456b21140bb2e2f113122e2",
    ),
    (
        include_bytes!("cache_receipt_public_good/15.root.json"),
        "73747011d0857ada15479a16c4cae0f3ed03aac698b523b97e1de314ac9d9ca8",
    ),
    (
        include_bytes!("cache_receipt_public_good/798.timestamp.json"),
        "12ff8634dd593bf446c734e941266a68cc822f6950e7d9f73cb9f6fe7097d2c1",
    ),
    (
        include_bytes!("cache_receipt_public_good/165.snapshot.json"),
        "8f784ab614ec62bfdd5f568eb2a2e3011668449ba235ed4eb7befa99f8469933",
    ),
    (
        include_bytes!("cache_receipt_public_good/14.targets.json"),
        "6a697f7f8908c8ab26c11786ecb490b54acec97fa8c802e399f065f8a0cc1acd",
    ),
    (
        include_bytes!("cache_receipt_public_good/public_release_bundle.json"),
        "b05db30dbcf69f6df8edcc5c71951a2a1429674520b480a5001890706e3976e5",
    ),
    (
        include_bytes!("cache_receipt_public_good/public_release_verification.json"),
        "aeb3fde11f3a295c0d8daf933f33c2bc306761d9d49632075511d467944a1f7f",
    ),
];

/// Private construction binds one reviewed Public Good root and its proof.
/// This grants no producer, protected-source, private-platform or CA authority.
#[derive(Debug, Clone)]
pub(crate) struct QualifiedPublicReceiptRoot {
    raw_bytes: &'static [u8],
    verifier_bytes: &'static [u8],
    qualification_bytes: &'static [u8],
}

impl QualifiedPublicReceiptRoot {
    /// Exact signed TUF target; historical certificate/log keys stay present.
    pub(crate) fn raw_bytes(&self) -> &'static [u8] {
        self.raw_bytes
    }

    /// Exact compact JSONL emitted by the qualified Linux GH TUF client.
    /// GH's custom trusted-root loader requires each root on one line.
    pub(crate) fn verifier_bytes(&self) -> &'static [u8] {
        self.verifier_bytes
    }

    pub(crate) fn raw_sha256(&self) -> &'static str {
        RAW_SHA256
    }

    pub(crate) fn verifier_sha256(&self) -> &'static str {
        VERIFIER_SHA256
    }

    pub(crate) fn qualification_sha256(&self) -> &'static str {
        QUALIFICATION_SHA256
    }

    /// Source-owned audit record; no runtime JSON loader interprets this record.
    pub(crate) fn qualification_bytes(&self) -> &'static [u8] {
        self.qualification_bytes
    }

    /// Platform visibility must come from independently authenticated evidence.
    /// Standard private/internal attest action bundles use a different instance.
    pub(crate) fn supports_visibility(&self, visibility: &str) -> bool {
        visibility == "public"
    }
}

/// Fixed source factory only; no caller arguments or arbitrary root constructors.
/// A changed compiled evidence byte rejects until the owner reviews new pins.
pub(crate) fn qualified_public_receipt_root() -> Option<QualifiedPublicReceiptRoot> {
    if EVIDENCE
        .iter()
        .any(|(bytes, expected)| compiled_source_sha256(bytes) != *expected)
    {
        return None;
    }
    Some(QualifiedPublicReceiptRoot {
        raw_bytes: RAW_BYTES,
        verifier_bytes: VERIFIER_BYTES,
        qualification_bytes: QUALIFICATION_BYTES,
    })
}

use crate::canonical::digest_b3;

/// Parse/serde round-trip must accept `good` and reject `bad`.
macro_rules! check_id {
    ($t:ty, $good:expr, $bad:expr) => {{
        let parsed = <$t>::parse($good).expect("valid id");
        assert_eq!(parsed.as_str(), $good);
        assert!(<$t>::parse($bad).is_err(), "accepted {}", $bad);
        let json = serde_json::to_string(&parsed).expect("serialize");
        let back = serde_json::from_str::<$t>(&json).expect("deserialize");
        assert_eq!(back, parsed);
        let bad_json = format!("\"{}\"", $bad);
        assert!(
            serde_json::from_str::<$t>(&bad_json).is_err(),
            "serde {bad_json}"
        );
    }};
}

/// Sample manifest digest for proof tests.
fn proof_digest() -> String {
    digest_b3(b"manifest")
}

/// Sample source commit for proof tests.
fn proof_commit() -> String {
    "ab".repeat(20)
}

mod newtypes;
mod proofs;

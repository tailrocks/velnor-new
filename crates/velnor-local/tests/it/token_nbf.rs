// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_local::token::{Claims, verify};

use crate::support::{MAIN, token, token_key};

const NBF: u64 = 1_794_000_000;
const EXP: u64 = 4_102_444_800;

#[test]
fn a_token_before_its_not_before_time_is_refused() {
    let refused = verify(&token(MAIN), &token_key(), NBF - 1);
    assert_eq!(
        refused,
        Err("the runtime token is not valid yet".to_owned())
    );
}

#[test]
fn a_token_is_accepted_from_its_not_before_time_until_it_expires() {
    for now in [NBF, NBF + 1, EXP - 1] {
        let claims = verify(&token(MAIN), &token_key(), now);
        assert_eq!(claims.map(|c| c.nbf), Ok(NBF), "now = {now}");
    }
    assert_eq!(
        verify(&token(MAIN), &token_key(), EXP),
        Err("the runtime token has expired".to_owned())
    );
}

#[test]
fn claims_without_a_numeric_not_before_are_malformed() {
    let base = r#""repository":"o/r","ref":"refs/heads/main","defaultRef":"refs/heads/main","exp":2,"scp":"s""#;
    for extra in ["", r#","nbf":"1""#] {
        let parsed = serde_json::from_str::<Claims>(&format!("{{{base}{extra}}}"));
        assert!(parsed.is_err(), "{extra}");
    }
}

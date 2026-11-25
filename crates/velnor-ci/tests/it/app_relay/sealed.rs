// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

const TOKEN: &str = "ghs_sealedInstallationToken0123456789";

fn binding() -> Binding {
    Binding::new("acme/app", 77, 1, "release-pr").unwrap_or_else(|e| unreachable!("{e}"))
}

fn sealed_for(binding: &Binding) -> (zeroize::Zeroizing<[u8; 32]>, String) {
    let (secret, document) = generate(&mut OsRng, binding);
    let json = document.to_json().unwrap_or_else(|e| unreachable!("{e}"));
    let parsed =
        PublicKeyDocument::parse(json.as_bytes(), binding).unwrap_or_else(|e| unreachable!("{e}"));
    let sealed = seal(&mut OsRng, &parsed, TOKEN).unwrap_or_else(|e| unreachable!("{e}"));
    (secret, sealed)
}

#[test]
fn the_sealed_token_opens_with_the_key_of_the_job_and_only_there() {
    let (secret, sealed) = sealed_for(&binding());
    assert!(!sealed.contains(TOKEN));
    let opened = open(&secret, &sealed, &binding()).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(opened.as_str(), TOKEN);
    let (other, _) = sealed_for(&binding());
    assert!(open(&other, &sealed, &binding()).is_err());
}

#[test]
fn a_token_sealed_for_another_run_attempt_job_or_repository_is_refused() {
    let (secret, sealed) = sealed_for(&binding());
    let others = [
        Binding::new("acme/app", 78, 1, "release-pr"),
        Binding::new("acme/app", 77, 2, "release-pr"),
        Binding::new("acme/app", 77, 1, "tag"),
        Binding::new("acme/other", 77, 1, "release-pr"),
    ];
    for other in others {
        let other = other.unwrap_or_else(|e| unreachable!("{e}"));
        let error = open(&secret, &sealed, &other).err().map(|e| e.to_string());
        assert!(
            error.is_some_and(|text| text.contains("is not the")),
            "{other:?} was accepted"
        );
    }
}

#[test]
fn a_public_key_bound_to_another_job_is_refused_before_anything_is_sealed() {
    let (_, document) = generate(&mut OsRng, &binding());
    let json = document.to_json().unwrap_or_else(|e| unreachable!("{e}"));
    let other = Binding::new("acme/app", 77, 1, "tag").unwrap_or_else(|e| unreachable!("{e}"));
    assert!(PublicKeyDocument::parse(json.as_bytes(), &other).is_err());
}

#[test]
fn a_tampered_sealed_token_is_refused() {
    let (secret, sealed) = sealed_for(&binding());
    let mut bytes = sealed.into_bytes();
    let middle = bytes.len() / 2;
    bytes[middle] = if bytes[middle] == b'A' { b'B' } else { b'A' };
    let tampered = String::from_utf8(bytes).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(open(&secret, &tampered, &binding()).is_err());
}

#[test]
fn a_job_identity_with_a_path_or_space_is_not_a_job_identity() {
    for job in ["", "a/b", "a b", "../x"] {
        assert!(Binding::new("acme/app", 1, 1, job).is_err(), "{job}");
    }
}

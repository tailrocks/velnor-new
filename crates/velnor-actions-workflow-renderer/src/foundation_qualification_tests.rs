use super::*;

const REFERENCE: &str =
    "tailrocks/velnor-new/foundation-qualification@8758d976a1b25eb387f48aa04ea86f57739b84cf";

#[test]
fn source_template_is_exact_reviewed_publication_owner_bytes() {
    assert_eq!(sha256_hex(TEMPLATE.as_bytes()), TEMPLATE_SHA256);
    assert_eq!(TEMPLATE.matches(ACTION_MARKER).count(), 1);
}

#[test]
fn native_qualification_retains_first_action_and_exact_fixed_policy() {
    let action = SourceActionReference::new(REFERENCE).expect("neutral pinned reference");
    let file = render(&action, "0.1.0").expect("source-only document");
    assert_eq!(file.path, WORKFLOW_PATH);
    assert_eq!(file.bytes.len(), 1159);
    assert_eq!(
        sha256_hex(file.bytes.as_bytes()),
        "c777fb6c4837da2fd38fe4ac68b5235b759fb628de68a22c6adc14153ba086a3"
    );
    marker::check_first_line(&file.bytes, "0.1.0").expect("explicit version marker");
    let body = file.bytes.split_once('\n').expect("marker").1;
    assert_eq!(body, TEMPLATE.replace(ACTION_MARKER, REFERENCE));
    let first_step = body.find("      - name:").expect("first step");
    let second_step = body[first_step + 1..]
        .find("      - name:")
        .map(|index| index + first_step + 1)
        .expect("upload step");
    let first = &body[first_step..second_step];
    assert!(first.contains(REFERENCE));
    assert!(!first.contains("run:"));
    assert!(!first.contains("checkout@"));
    assert!(!first.contains("cache@"));
    assert!(body.contains("runs-on: ubuntu-26.04"));
    assert!(body.contains("permissions:\n  contents: read\n"));
    assert!(body.contains("on:\n  workflow_dispatch:\n"));
    assert!(body.contains("actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a"));
    assert!(!body.contains(ACTION_MARKER));
}

#[test]
fn mutated_template_rejects_before_render() {
    let action = SourceActionReference::new(REFERENCE).expect("neutral pinned reference");
    for template in [
        TEMPLATE.replace("ubuntu-26.04", "ubuntu-latest"),
        TEMPLATE.replace(ACTION_MARKER, "caller/repository/action@main"),
        format!("{TEMPLATE}\nuses: {ACTION_MARKER}\n"),
    ] {
        assert!(render_compiled(&action, "0.1.0", &template).is_err());
    }
}

#[test]
fn neutral_reference_and_explicit_version_reject_serialization_injection() {
    for reference in [
        "caller/repository/action@main".to_owned(),
        format!("caller/repository/action@{}", "A".repeat(40)),
        format!("caller/repository/action@{}\nrun: injected", "a".repeat(40)),
        format!("caller/repository/../action@{}", "a".repeat(40)),
        format!("caller//action@{}", "a".repeat(40)),
    ] {
        assert!(SourceActionReference::new(&reference).is_err());
    }
    let action = SourceActionReference::new(REFERENCE).expect("neutral pinned reference");
    assert!(render(&action, "0.1.0\nrun: injected").is_err());
}

#[test]
fn private_digest_matches_known_sha256_vectors() {
    assert_eq!(
        sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

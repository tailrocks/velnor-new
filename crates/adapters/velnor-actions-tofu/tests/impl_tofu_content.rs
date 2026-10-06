//! E4 content-signal cases.
use velnor_actions_tofu_core::content::signals_for;
use velnor_actions_tofu_core::effective::Dialect;

#[test]
fn native_version_and_legacy_signal_together() {
    let signals = signals_for(
        "terraform {\n  required_version = \">= 1.6\"\n}\nresource \"t\" \"n\" {\n  cmd = \"terraform plan\"\n}\n",
        Dialect::Native,
    )
    .expect("signals");
    assert_eq!(signals.required_versions, vec![">= 1.6"]);
    assert!(signals.has_legacy_ref);
    assert!(!signals.terraform_only);
}

#[test]
fn terraform_only_pin_flags() {
    let signals = signals_for(
        "terraform {\n  required_version = \"= 1.5.7\"\n}\n",
        Dialect::Native,
    )
    .expect("signals");
    assert!(signals.terraform_only);
    assert!(!signals.has_legacy_ref);
}

#[test]
fn empty_file_signals_nothing() {
    let signals = signals_for("", Dialect::Native).expect("signals");
    assert!(signals.required_versions.is_empty());
    assert!(!signals.has_legacy_ref);
    assert!(!signals.terraform_only);
}

#[test]
fn non_string_version_ignored() {
    let signals = signals_for(
        "terraform {\n  required_version = var.pinned\n}\n",
        Dialect::Native,
    )
    .expect("signals");
    assert!(signals.required_versions.is_empty());
}

#[test]
fn malformed_native_propagates() {
    assert!(signals_for("variable \"x\" {", Dialect::Native).is_err());
}

#[test]
fn json_version_and_legacy_signal_together() {
    let signals = signals_for(
        "{\"terraform\": {\"required_version\": \"~> 1.5.0\"}, \"locals\": {\"cmd\": \"terraform fmt\"}}",
        Dialect::Json,
    )
    .expect("signals");
    assert_eq!(signals.required_versions, vec!["~> 1.5.0"]);
    assert!(signals.has_legacy_ref);
    assert!(signals.terraform_only);
}

#[test]
fn json_non_object_propagates() {
    assert!(signals_for("[1]", Dialect::Json).is_err());
}

#[test]
fn json_malformed_propagates() {
    assert!(signals_for("{\"a\": }", Dialect::Json).is_err());
}

#[test]
fn multiple_versions_any_pin_flags() {
    let signals = signals_for(
        "terraform {\n  required_version = \">= 1.6\"\n}\nterraform {\n  required_version = \"< 1.6.0\"\n}\n",
        Dialect::Native,
    )
    .expect("signals");
    assert_eq!(signals.required_versions.len(), 2);
    assert!(signals.terraform_only);
}

#[test]
fn native_labels_never_signal() {
    let signals = signals_for(
        "resource \"terraform_data\" \"x\" {}\ndata \"terraform_remote_state\" \"y\" {}\n",
        Dialect::Native,
    )
    .expect("signals");
    assert!(!signals.has_legacy_ref);
    assert!(signals.required_versions.is_empty());
}

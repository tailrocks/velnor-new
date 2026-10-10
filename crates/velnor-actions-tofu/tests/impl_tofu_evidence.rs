//! Dialect evidence classification cases.
use std::collections::BTreeMap;
use velnor_actions_tofu::evidence::{
    EvidenceLevel, TofuNote, classify, classify_with_contents, mise_tool_selected, plan_note,
};

/// Index file list from names.
fn files(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

/// Flattened mise.toml values from dotted keys.
fn mise(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
    entries
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect()
}

#[test]
fn empty_repo_classifies_none_silently() {
    let evidence = classify(&files(&["src/lib.rs", "Cargo.toml"]), &mise(&[]));
    assert_eq!(evidence.level, EvidenceLevel::None);
    assert_eq!(evidence.signals, [] as [String; 0]);
    assert_eq!(evidence.inferred, [] as [String; 0]);
    assert_eq!(plan_note(&evidence), None);
}

#[test]
fn tf_only_classifies_weak_with_sightings() {
    let evidence = classify(&files(&["main.tf", "vars.tf.json"]), &mise(&[]));
    assert_eq!(evidence.level, EvidenceLevel::Weak);
    assert!(evidence.signals.contains(&"legacy-only:main.tf".to_owned()));
    assert!(
        evidence
            .signals
            .contains(&"legacy-only:vars.tf.json".to_owned())
    );
    let note = plan_note(&evidence).expect("weak advises");
    assert!(matches!(note, TofuNote::Advisory(advisory) if !advisory.strong));
}

#[test]
fn tofu_spelling_classifies_strong() {
    let evidence = classify(&files(&["main.tf", "extra.tofu"]), &mise(&[]));
    assert_eq!(evidence.level, EvidenceLevel::Strong);
    assert!(
        evidence
            .signals
            .contains(&"tofu-spelling:extra.tofu".to_owned())
    );
    assert!(
        !evidence
            .signals
            .iter()
            .any(|signal| signal.starts_with("legacy-only"))
    );
    let note = plan_note(&evidence).expect("strong advises");
    assert!(matches!(note, TofuNote::Advisory(advisory) if advisory.strong));
}

#[test]
fn tofu_json_spelling_classifies_strong() {
    let evidence = classify(&files(&["v.tofu.json"]), &mise(&[]));
    assert_eq!(evidence.level, EvidenceLevel::Strong);
}

#[test]
fn mise_opentofu_tool_classifies_strong() {
    for entries in [
        vec![("tools.opentofu", "1.13.1")],
        vec![("tools.opentofu.version", "1.13.1")],
    ] {
        let evidence = classify(&files(&["src/lib.rs"]), &mise(&entries));
        assert_eq!(evidence.level, EvidenceLevel::Strong);
        assert!(evidence.signals.contains(&"mise-tool:opentofu".to_owned()));
    }
}

#[test]
fn mise_selection_matches_exact_tool_segments() {
    let values = mise(&[("tools.rust", "1.98.1"), ("tools.my-opentofu-fork", "1.0")]);
    assert!(!mise_tool_selected(&values, "opentofu"));
    assert!(mise_tool_selected(&values, "rust"));
    assert!(!mise_tool_selected(&mise(&[]), "opentofu"));
}

#[test]
fn terraform_tool_plus_strong_conflicts() {
    let values = mise(&[("tools.opentofu", "1.13.1"), ("tools.terraform", "1.9.0")]);
    let evidence = classify(&files(&["main.tf"]), &values);
    assert_eq!(evidence.level, EvidenceLevel::Conflict);
    assert!(evidence.signals.contains(&"mise-tool:opentofu".to_owned()));
    assert!(
        evidence
            .signals
            .contains(&"terraform-marker:mise-tool:terraform".to_owned()),
        "{:?}",
        evidence.signals
    );
    assert_eq!(plan_note(&evidence), None, "conflict errors, never notes");
}

#[test]
fn terraform_d_dir_plus_strong_conflicts() {
    let evidence = classify(&files(&["main.tofu", ".terraform.d/plugins/x"]), &mise(&[]));
    assert_eq!(evidence.level, EvidenceLevel::Conflict);
    let evidence = classify(&files(&["main.tofu", "nested/.terraform.d/y"]), &mise(&[]));
    assert_eq!(evidence.level, EvidenceLevel::Conflict);
}

#[test]
fn terraform_markers_alone_stay_none() {
    let values = mise(&[("tools.terraform", "1.9.0")]);
    let evidence = classify(&files(&["main.tf"]), &values);
    assert_eq!(evidence.level, EvidenceLevel::None, "terraform's territory");
    assert_eq!(plan_note(&evidence), None);
}

#[test]
fn terraform_working_dirs_are_not_markers() {
    let evidence = classify(
        &files(&["main.tofu", ".terraform/providers/x.zip"]),
        &mise(&[]),
    );
    assert_eq!(evidence.level, EvidenceLevel::Strong);
    assert!(
        !evidence
            .signals
            .iter()
            .any(|signal| signal.contains("terraform-marker"))
    );
}

#[test]
fn single_effective_dir_infers_advisory_roots() {
    let evidence = classify(&files(&["infra/main.tf", "infra/vars.tf"]), &mise(&[]));
    assert_eq!(evidence.level, EvidenceLevel::Weak);
    assert_eq!(evidence.inferred, files(&["infra"]));
    let evidence = classify(&files(&["main.tf"]), &mise(&[]));
    assert_eq!(evidence.inferred, files(&["."]));
}

#[test]
fn ambiguous_or_empty_dirs_infer_nothing() {
    let evidence = classify(&files(&["a/main.tf", "b/main.tf"]), &mise(&[]));
    assert_eq!(evidence.inferred, [] as [String; 0]);
    let evidence = classify(&files(&["notes.txt"]), &mise(&[]));
    assert_eq!(evidence.inferred, [] as [String; 0]);
    // Shadowed pairs still locate their directory.
    let evidence = classify(&files(&["infra/main.tf", "infra/main.tofu"]), &mise(&[]));
    assert_eq!(evidence.inferred, files(&["infra"]));
}

#[test]
fn sightings_cap_with_overflow_entry() {
    let names: Vec<String> = (0..20).map(|n| format!("d{n}/f.tf")).collect();
    let evidence = classify(&names, &mise(&[]));
    assert_eq!(evidence.level, EvidenceLevel::Weak);
    assert!(
        evidence.signals.contains(&"more:legacy-only:12".to_owned()),
        "{:?}",
        evidence.signals
    );
    assert_eq!(evidence.signals.len(), 9);
    // Twenty dirs stay ambiguous.
    assert_eq!(evidence.inferred, [] as [String; 0]);
}

#[test]
fn test_and_var_files_are_not_evidence() {
    let evidence = classify(
        &files(&["check.tftest.hcl", "terraform.tfvars", "x.auto.tfvars"]),
        &mise(&[]),
    );
    assert_eq!(evidence.level, EvidenceLevel::None);
}

/// Bounded contents map from path/text entries.
fn contents(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
    entries
        .iter()
        .map(|(path, text)| ((*path).to_owned(), (*text).to_owned()))
        .collect()
}

#[test]
fn version_plus_legacy_is_strong() {
    use velnor_actions_tofu::evidence::classify_with_contents;
    let evidence = classify_with_contents(
        &files(&["main.tf"]),
        &contents(&[(
            "main.tf",
            "terraform {\n  required_version = \">= 1.6\"\n}\nresource \"t\" \"n\" {\n  cmd = \"terraform plan\"\n}\n",
        )]),
        &mise(&[]),
    );
    assert_eq!(evidence.level, EvidenceLevel::Strong);
    assert!(
        evidence
            .signals
            .iter()
            .any(|signal| signal.starts_with("content:required-version:")),
        "{:?}",
        evidence.signals
    );
    assert!(
        evidence
            .signals
            .iter()
            .any(|signal| signal.starts_with("content:legacy-ref:")),
        "{:?}",
        evidence.signals
    );
}

#[test]
fn version_alone_stays_weak() {
    use velnor_actions_tofu::evidence::classify_with_contents;
    let evidence = classify_with_contents(
        &files(&["main.tf"]),
        &contents(&[(
            "main.tf",
            "terraform {\n  required_version = \">= 1.6\"\n}\n",
        )]),
        &mise(&[]),
    );
    assert_eq!(evidence.level, EvidenceLevel::Weak);
    assert!(
        evidence
            .signals
            .iter()
            .any(|signal| signal.starts_with("content:required-version:")),
        "{:?}",
        evidence.signals
    );
}

#[test]
fn legacy_alone_stays_weak() {
    use velnor_actions_tofu::evidence::classify_with_contents;
    let evidence = classify_with_contents(
        &files(&["main.tf"]),
        &contents(&[("main.tf", "locals {\n  cmd = \"terraform plan\"\n}\n")]),
        &mise(&[]),
    );
    assert_eq!(evidence.level, EvidenceLevel::Weak);
}

#[test]
fn terraform_only_pin_is_a_marker() {
    use velnor_actions_tofu::evidence::classify_with_contents;
    let evidence = classify_with_contents(
        &files(&["main.tofu"]),
        &contents(&[(
            "main.tofu",
            "terraform {\n  required_version = \"= 1.5.7\"\n}\n",
        )]),
        &mise(&[]),
    );
    assert_eq!(evidence.level, EvidenceLevel::Conflict);
    assert!(
        evidence
            .signals
            .iter()
            .any(|signal| signal.starts_with("terraform-marker:required-version:")),
        "{:?}",
        evidence.signals
    );
}

#[test]
fn terraform_only_pin_alone_stays_silent() {
    use velnor_actions_tofu::evidence::classify_with_contents;
    let evidence = classify_with_contents(
        &files(&["main.tf"]),
        &contents(&[(
            "main.tf",
            "terraform {\n  required_version = \"= 1.5.7\"\n}\n",
        )]),
        &mise(&[]),
    );
    assert_eq!(evidence.level, EvidenceLevel::None);
}

#[test]
fn malformed_content_contributes_no_signals() {
    use velnor_actions_tofu::evidence::classify_with_contents;
    let evidence = classify_with_contents(
        &files(&["main.tf"]),
        &contents(&[("main.tf", "variable \"x\" {")]),
        &mise(&[]),
    );
    assert_eq!(evidence.level, EvidenceLevel::Weak);
    assert!(
        evidence
            .signals
            .iter()
            .all(|signal| !signal.starts_with("content:")),
        "{:?}",
        evidence.signals
    );
}

#[test]
fn non_config_content_ignored() {
    use velnor_actions_tofu::evidence::classify_with_contents;
    let evidence = classify_with_contents(
        &files(&["main.tf"]),
        &contents(&[(
            "notes.txt",
            "terraform {\n  required_version = \"= 1.5.7\"\n}\n",
        )]),
        &mise(&[]),
    );
    assert_eq!(evidence.level, EvidenceLevel::Weak);
}

#[test]
fn json_content_signals_fire() {
    use velnor_actions_tofu::evidence::classify_with_contents;
    let evidence = classify_with_contents(
        &files(&["main.tf"]),
        &contents(&[(
            "main.tf",
            "terraform {\n  required_version = \">= 1.6\"\n}\n",
        )]),
        &mise(&[]),
    );
    assert_eq!(evidence.level, EvidenceLevel::Weak);
    let evidence = classify_with_contents(
        &files(&["config.tf.json"]),
        &contents(&[(
            "config.tf.json",
            "{\"terraform\": {\"required_version\": \">= 1.6\"}, \"locals\": {\"cmd\": \"terraform plan\"}}",
        )]),
        &mise(&[]),
    );
    assert_eq!(evidence.level, EvidenceLevel::Strong);
}

#[test]
fn bare_classify_reads_no_content() {
    let evidence = classify(&files(&["main.tf"]), &mise(&[]));
    assert_eq!(evidence.level, EvidenceLevel::Weak);
    assert!(
        evidence
            .signals
            .iter()
            .all(|signal| !signal.starts_with("content:")),
        "{:?}",
        evidence.signals
    );
}

#[test]
fn e4_strong_advises_in_plan_note() {
    let evidence = classify_with_contents(
        &files(&["main.tf"]),
        &contents(&[(
            "main.tf",
            "terraform {\n  required_version = \">= 1.6\"\n}\nlocals {\n  cmd = \"terraform plan\"\n}\n",
        )]),
        &mise(&[]),
    );
    match plan_note(&evidence) {
        Some(TofuNote::Advisory(advisory)) => assert!(advisory.strong),
        other => panic!("expected strong advisory, got {other:?}"),
    }
}

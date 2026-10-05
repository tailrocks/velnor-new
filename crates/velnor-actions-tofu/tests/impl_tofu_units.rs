//! Unit-analysis cases: precedence, structure, duplicates, scopes.
use velnor_actions_tofu::modules::ModuleSource;
use velnor_actions_tofu::units::{
    UnitError, analyze_files, files_for_prefix, module_refs_for_texts,
};

use crate::support::{Outcome, fixture_dir, read_pairs};

/// `(path, text)` pairs from inline entries.
fn pairs(entries: &[(&str, &str)]) -> Vec<(String, String)> {
    entries
        .iter()
        .map(|(path, text)| ((*path).to_owned(), (*text).to_owned()))
        .collect()
}

#[test]
fn effective_set_drops_shadowed_before_parsing() {
    let unit = analyze_files(&pairs(&[
        ("main.tf", "((( garbage"),
        ("main.tofu", "variable \"x\" {}\n"),
        ("extra.tf", "output \"y\" {\n  value = 1\n}\n"),
    ]))
    .expect("shadowed garbage never parses");
    assert_eq!(unit.effective, vec!["extra.tf", "main.tofu"]);
    assert_eq!(unit.fmt, vec!["extra.tf", "main.tf", "main.tofu"]);
}

#[test]
fn duplicate_variables_error_across_files() {
    let err = analyze_files(&pairs(&[
        ("a.tf", "variable \"dup\" {}\n"),
        ("b.tf", "variable \"dup\" {}\n"),
    ]))
    .expect_err("dup errors");
    assert!(matches!(err, UnitError::Duplicate { .. }), "{err}");
    assert!(err.to_string().contains("a.tf"), "{err}");
    assert!(err.to_string().contains("b.tf"), "{err}");
}

#[test]
fn duplicate_within_one_file_errors() {
    let err = analyze_files(&pairs(&[(
        "a.tf",
        "variable \"d\" {}\nvariable \"d\" {}\n",
    )]))
    .expect_err("dup errors");
    assert!(matches!(err, UnitError::Duplicate { .. }), "{err}");
}

#[test]
fn overrides_repeat_freely() {
    let unit = analyze_files(&pairs(&[
        ("main.tf", "variable \"level\" {}\n"),
        ("a_override.tf", "variable \"level\" {}\n"),
        ("b_override.tf", "variable \"level\" {}\n"),
    ]))
    .expect("overrides exempt");
    assert_eq!(unit.effective.len(), 3);
}

#[test]
fn unknown_blocks_error_in_base_and_override() {
    for name in ["main.tf", "z_override.tf"] {
        let err = analyze_files(&pairs(&[(name, "frobnicate \"x\" {}\n")])).expect_err("unknown");
        assert!(matches!(err, UnitError::UnknownBlock { .. }), "{err}");
    }
}

#[test]
fn bad_label_counts_error_on_tracked_kinds() {
    let err = analyze_files(&pairs(&[("a.tf", "resource \"lonely\" {}\n")])).expect_err("shape");
    assert!(matches!(err, UnitError::Shape { .. }), "{err}");
    let err = analyze_files(&pairs(&[("a.tf", "variable \"a\" \"b\" {}\n")])).expect_err("shape");
    assert!(matches!(err, UnitError::Shape { .. }), "{err}");
}

#[test]
fn cross_dialect_doubles_error() {
    let err = analyze_files(&pairs(&[
        ("main.tf", "variable \"clash\" {}\n"),
        ("clash.tf.json", "{\"variable\": {\"clash\": {}}}"),
    ]))
    .expect_err("clash errors");
    assert!(matches!(err, UnitError::Duplicate { .. }), "{err}");
}

#[test]
fn child_directories_keep_their_own_namespace() {
    analyze_files(&pairs(&[
        ("main.tf", "variable \"same\" {}\n"),
        ("modules/child/main.tf", "variable \"same\" {}\n"),
    ]))
    .expect("per-dir namespaces");
}

#[test]
fn files_for_prefix_caps_the_selection() {
    let files: Vec<String> = (0..2000).map(|index| format!("f{index}.tf")).collect();
    let err = files_for_prefix(&files, "").expect_err("over cap");
    assert!(matches!(err, UnitError::TooManyFiles { .. }), "{err}");
    let scoped =
        files_for_prefix(&["a.tf".to_owned(), "sub/b.tf".to_owned()], "sub").expect("prefix");
    assert_eq!(scoped, vec!["sub/b.tf"]);
}

#[test]
fn precedence_fixture_loads_and_formats() -> Outcome {
    let all = read_pairs(&fixture_dir("tofu-precedence"), "")?;
    let root: Vec<(String, String)> = all
        .into_iter()
        .filter(|(path, _)| !path.contains('/'))
        .collect();
    let unit = analyze_files(&root).expect("precedence analyzes");
    assert_eq!(unit.effective, vec!["extra.tf", "main.tofu"]);
    assert_eq!(unit.fmt, vec!["extra.tf", "main.tf", "main.tofu"]);
    let control = read_pairs(&fixture_dir("tofu-precedence/dup-control"), "dup-control")?;
    let err = analyze_files(&control).expect_err("control dups");
    assert!(matches!(err, UnitError::Duplicate { .. }), "{err}");
    Ok(())
}

#[test]
fn override_fixture_shadows_garbage_and_repeats() -> Outcome {
    let all = read_pairs(&fixture_dir("tofu-override"), "")?;
    let root: Vec<(String, String)> = all
        .into_iter()
        .filter(|(path, _)| !path.contains('/'))
        .collect();
    let unit = analyze_files(&root).expect("override analyzes");
    assert_eq!(
        unit.effective,
        vec![
            "a_override.tf",
            "b_override.tf",
            "c_override.tofu",
            "main.tf",
            "override.tofu",
        ]
    );
    assert!(unit.fmt.contains(&"c_override.tf".to_owned()));
    assert!(unit.fmt.contains(&"override.tf".to_owned()));
    let proof = read_pairs(&fixture_dir("tofu-override/load-proof"), "load-proof")?;
    let err = analyze_files(&proof).expect_err("override unknown block");
    assert!(matches!(err, UnitError::UnknownBlock { .. }), "{err}");
    Ok(())
}

#[test]
fn json_and_malformed_fixtures_behave() -> Outcome {
    let valid = read_pairs(&fixture_dir("tofu-json/valid"), "valid")?;
    analyze_files(&valid).expect("valid json analyzes");
    let clash = read_pairs(&fixture_dir("tofu-json/clash"), "clash")?;
    assert!(matches!(
        analyze_files(&clash).expect_err("clash"),
        UnitError::Duplicate { .. }
    ));
    for (case, variant) in [
        ("hcl-syntax", "Parse"),
        ("json-syntax", "Parse"),
        ("hcl-semantic", "UnknownBlock"),
        ("dup-var", "Duplicate"),
    ] {
        let dir = fixture_dir(&format!("tofu-malformed/{case}"));
        let err = analyze_files(&read_pairs(&dir, case)?).expect_err(case);
        let text = err.to_string();
        let kind = match err {
            UnitError::Parse { .. } => "Parse",
            UnitError::UnknownBlock { .. } => "UnknownBlock",
            UnitError::Duplicate { .. } => "Duplicate",
            _ => "other",
        };
        assert_eq!(kind, variant, "{case}: {text}");
    }
    let control = read_pairs(&fixture_dir("tofu-malformed/control"), "control")?;
    analyze_files(&control).expect("control analyzes");
    Ok(())
}

#[test]
fn consumer_mirror_analyzes_clean() -> Outcome {
    let all = read_pairs(&fixture_dir("tofu-consumer-mirror"), "")?;
    let unit = analyze_files(&all).expect("consumer mirror analyzes");
    assert!(!unit.effective.is_empty());
    assert_eq!(unit.effective.len(), unit.fmt.len());
    Ok(())
}

#[test]
fn analyze_extracts_module_refs() {
    let unit = analyze_files(&pairs(&[
        ("main.tf", "module \"a\" {\n  source = \"./mods/a\"\n}\n"),
        (
            "extra.tf.json",
            "{\"module\": {\"b\": {\"source\": \"../shared\"}}}",
        ),
        (
            "z_override.tf",
            "module \"c\" {\n  source = \"ns/name/sys\"\n}\n",
        ),
    ]))
    .expect("analyzes");
    assert_eq!(unit.modules.len(), 3);
    let by_name = |name: &str| {
        unit.modules
            .iter()
            .find(|reference| reference.name == name)
            .expect("fixture ref present")
    };
    assert_eq!(by_name("a").file, "main.tf");
    assert!(matches!(
        &by_name("a").source,
        ModuleSource::Literal(source) if source == "./mods/a"
    ));
    assert_eq!(by_name("b").file, "extra.tf.json");
    assert!(matches!(
        &by_name("b").source,
        ModuleSource::Literal(source) if source == "../shared"
    ));
    assert_eq!(by_name("c").file, "z_override.tf");
    assert!(matches!(
        &by_name("c").source,
        ModuleSource::Literal(source) if source == "ns/name/sys"
    ));
}

#[test]
fn shadowed_module_calls_ignored() {
    let unit = analyze_files(&pairs(&[
        ("main.tf", "module \"ghost\" {\n  source = \"./ghost\"\n}\n"),
        ("main.tofu", "variable \"x\" {}\n"),
    ]))
    .expect("analyzes");
    assert!(unit.modules.is_empty(), "shadowed never parsed");
}

#[test]
fn fmt_scope_covers_tfvars_but_never_json() -> Outcome {
    let tfvars = read_pairs(&fixture_dir("tofu-tfvars"), "")?;
    let unit = analyze_files(&tfvars).expect("tfvars analyzes");
    assert_eq!(unit.effective, vec!["main.tf"]);
    assert!(unit.fmt.contains(&"custom.tfvars".to_owned()));
    assert!(unit.fmt.contains(&"terraform.tfvars".to_owned()));
    assert!(!unit.fmt.iter().any(|path| path.ends_with(".example")));
    Ok(())
}

#[test]
fn refs_extract_without_validation_checks() {
    let refs = module_refs_for_texts(&pairs(&[
        ("main.tf", "module \"a\" {\n  source = \"./mods/a\"\n}\n"),
        (
            "extra.tf.json",
            "{\"module\": {\"b\": {\"source\": \"../shared\"}}}",
        ),
        // Duplicates and unknown blocks pass: history predates checks.
        (
            "dup.tf",
            "variable \"x\" {}\nvariable \"x\" {}\nwat \"q\" {}\n",
        ),
    ]))
    .expect("refs extract");
    assert_eq!(refs.len(), 2);
    assert!(refs.iter().any(|reference| reference.name == "a"));
    assert!(refs.iter().any(|reference| reference.name == "b"));
}

#[test]
fn refs_reject_non_config_and_garbage() {
    let err = module_refs_for_texts(&pairs(&[("notes.md", "module \"a\" {}\n")]))
        .expect_err("non-config fails");
    assert!(err.to_string().contains("not_config"), "{err}");
    let err =
        module_refs_for_texts(&pairs(&[("main.tf", "((( garbage")])).expect_err("garbage fails");
    assert!(err.to_string().contains("malformed:main.tf"), "{err}");
}

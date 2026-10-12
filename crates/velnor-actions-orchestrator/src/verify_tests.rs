//! `[workflow.verify]` vector cases.

use super::*;
/// Config jobs in scrambled order.
fn scrambled() -> Vec<String> {
    [
        "native-validators",
        "link-check",
        "alint",
        "frontmatter-id",
        "markdownlint",
        "strict-json",
        "zizmor",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

#[test]
fn verify_kinds_emit_canonical_order() {
    let kinds = verify_kinds(&scrambled()).expect("known jobs resolve");
    let ids: Vec<&str> = kinds.iter().map(ValidatorKind::job_id).collect();
    assert_eq!(
        ids,
        [
            "zizmor",
            "alint",
            "markdownlint",
            "strict-json",
            "frontmatter-id",
            "link-check",
            "native-validators"
        ]
    );
    assert_eq!(verify_kinds(&[]).expect("empty resolves"), Vec::new());
    let err = verify_kinds(&["wat".to_owned()]).expect_err("unknown fails");
    assert!(err.to_string().contains("unknown_verify_job:wat"), "{err}");
}

#[test]
fn verify_vectors_carry_pins_and_payloads() {
    let catalog = ToolCatalog::pinned();
    let joined = |kind: ValidatorKind| {
        verify_command(kind, &VerifyConfig::default(), &catalog)
            .expect("command builds")
            .expect("shell kind has a command")
            .argv
            .join(" ")
    };
    let markdownlint = joined(ValidatorKind::Markdownlint);
    for token in [
        "npm:markdownlint-cli2@0.23.3",
        "markdownlint-cli2",
        "**/*.{md,markdown}",
        "#**/node_modules/**",
        "#.github/AGENTS.md",
        "#.github/CLAUDE.md",
    ] {
        assert!(markdownlint.contains(token), "{token}: {markdownlint}");
    }
    let strict = joined(ValidatorKind::StrictJson);
    assert!(strict.contains("node@24.21.0"), "{strict}");
    assert!(strict.contains("duplicate-key:"), "{strict}");
    let frontmatter = joined(ValidatorKind::FrontmatterId);
    assert!(frontmatter.contains("node@24.21.0"), "{frontmatter}");
    assert!(frontmatter.contains("name-dir-mismatch:"), "{frontmatter}");
    let links = joined(ValidatorKind::LinkCheck);
    for token in ["ubi:lycheeverse/lychee@0.15.1", "lychee", "--no-progress"] {
        assert!(links.contains(token), "{token}: {links}");
    }
    let native = verify_command(
        ValidatorKind::NativeValidators,
        &VerifyConfig::default(),
        &catalog,
    )
    .expect("command builds")
    .expect("native has a command")
    .argv;
    assert_eq!(native[0], "sh");
    assert_eq!(native[1], "-c");
    assert!(native[2].contains("claude plugin validate --strict"));
    assert!(
        verify_command(ValidatorKind::Alint, &VerifyConfig::default(), &catalog)
            .expect("alint resolves")
            .is_none(),
        "alint renders as an action step"
    );
    let err = verify_command(ValidatorKind::CargoDeny, &VerifyConfig::default(), &catalog)
        .expect_err("deny rejected");
    assert!(err.to_string().contains("verify_kind_rejected"), "{err}");
}

#[test]
fn configured_content_verifiers_retain_argument_boundaries() {
    let catalog = ToolCatalog::pinned();
    let options = VerifyConfig {
        jobs: vec!["strict-json".to_owned(), "link-check".to_owned()],
        strict_json_files: Some(vec![".github/marketplace.json".to_owned()]),
        lychee_args: vec![
            "--offline".to_owned(),
            "--include-fragments".to_owned(),
            "--exclude".to_owned(),
            "docs/**".to_owned(),
        ],
    };
    let strict = verify_command(ValidatorKind::StrictJson, &options, &catalog)
        .expect("strict JSON vector builds")
        .expect("strict JSON command exists");
    assert_eq!(
        strict.argv.last().map(String::as_str),
        Some("[\".github/marketplace.json\"]")
    );
    let links = verify_command(ValidatorKind::LinkCheck, &options, &catalog)
        .expect("link check vector builds")
        .expect("link check command exists");
    let position = links.argv.iter().position(|arg| arg == "lychee").unwrap();
    assert_eq!(
        &links.argv[position + 1..],
        [
            "--no-progress",
            "--offline",
            "--include-fragments",
            "--exclude",
            "docs/**",
            "**/*.md",
            "**/*.markdown"
        ]
    );
}

#[test]
fn frontmatter_parser_normalizes_multiline_yaml() {
    let Ok(node) = std::process::Command::new("node").arg("--version").output() else {
        return;
    };
    assert!(node.status.success());
    let temp = tempfile::tempdir().expect("temporary repository");
    let skills = temp.path().join("skills");
    for (name, description) in [
        (
            "folded",
            "> # comment\n  first line\n  second line\n\n  second paragraph\n",
        ),
        ("literal", "|-2\n  first line\n  second line\n"),
        ("literal-paragraph", "|-\n  first line\n\n  second line\n"),
        ("literal-leading", "|-\n\n  first line\n"),
        (
            "quoted",
            "\"A \\\"quoted\\\" # value\" # trailing comment\n",
        ),
        ("plain", "first line\n  second line\n\n  second paragraph\n"),
        ("modifier-order", ">-2\n  first line\n  second line\n"),
        ("literal-keep", "|+2\n  first line\n\n"),
    ] {
        let dir = skills.join(name);
        std::fs::create_dir_all(&dir).expect("skill directory");
        std::fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: {description}metadata: {{author: team}}\nallowed-tools: [read, grep]\n---\nBody.\n"),
        )
        .expect("skill file");
    }
    let instrumented = FRONTMATTER_ID_SCRIPT.replace(
        "const n = fm.get(\"name\");",
        "console.log(\"description:\" + JSON.stringify(fm.get(\"description\"))); const n = fm.get(\"name\");",
    );
    let valid = std::process::Command::new("node")
        .arg("-e")
        .arg(instrumented)
        .current_dir(temp.path())
        .output()
        .expect("run frontmatter validator");
    assert!(
        valid.status.success(),
        "{}",
        String::from_utf8_lossy(&valid.stdout)
    );
    let output = String::from_utf8_lossy(&valid.stdout);
    for expected in [
        r#"description:"first line second line\nsecond paragraph""#,
        r#"description:"first line\nsecond line""#,
        r#"description:"first line\n\nsecond line""#,
        r#"description:"\nfirst line""#,
        r#"description:"A \"quoted\" # value""#,
        r#"description:"first line second line""#,
        r#"description:"first line\n\n""#,
    ] {
        assert!(
            output.contains(expected),
            "missing {expected:?} in {output}"
        );
    }
}

#[test]
fn frontmatter_parser_rejects_duplicate_and_invalid_scalars() {
    if std::process::Command::new("node")
        .arg("--version")
        .output()
        .is_err()
    {
        return;
    }
    let temp = tempfile::tempdir().expect("temporary repository");
    let skills = temp.path().join("skills");
    let folded = skills.join("folded");
    std::fs::create_dir_all(&folded).expect("skill directory");
    let duplicate = folded.join("SKILL.md");
    std::fs::write(
        &duplicate,
        "---\nname: folded\ndescription: first\ndescription: second\n---\nBody.\n",
    )
    .expect("duplicate key fixture");
    let invalid = std::process::Command::new("node")
        .arg("-e")
        .arg(FRONTMATTER_ID_SCRIPT)
        .current_dir(temp.path())
        .output()
        .expect("run frontmatter validator");
    assert!(!invalid.status.success());
    assert!(String::from_utf8_lossy(&invalid.stdout).contains("duplicate-frontmatter-key"));

    std::fs::write(
        &duplicate,
        "---\nname: folded\ndescription: valid\n---\nBody.\n",
    )
    .expect("reset duplicate fixture");
    for (name, value) in [
        ("unclosed", "\"not closed"),
        ("collection", "[]"),
        ("multiline-quoted", "\"first line\n  second line\""),
        ("bad-indent", "|2\n x"),
        ("tagged-sequence", "!!seq [item]"),
    ] {
        let dir = skills.join(name);
        std::fs::create_dir_all(&dir).expect("invalid skill directory");
        std::fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: {value}\n---\nBody.\n"),
        )
        .expect("invalid skill file");
    }
    let malformed = std::process::Command::new("node")
        .arg("-e")
        .arg(FRONTMATTER_ID_SCRIPT)
        .current_dir(temp.path())
        .output()
        .expect("run frontmatter validator");
    let malformed_output = String::from_utf8_lossy(&malformed.stdout);
    assert!(!malformed.status.success());
    assert!(malformed_output.contains("bad-frontmatter-scalar"));
    assert!(malformed_output.contains("non-string-frontmatter-scalar"));
    assert!(malformed_output.contains("bad-frontmatter-indent"));
    assert!(malformed_output.contains("unsupported-frontmatter-alias-or-tag"));

    let alias = skills.join("alias/SKILL.md");
    std::fs::create_dir_all(alias.parent().expect("skill directory path"))
        .expect("alias skill directory");
    let long_value = "x".repeat(1025);
    std::fs::write(
        &alias,
        format!("---\nname: alias\npayload: &long {long_value}\ndescription: *long\n---\nBody.\n"),
    )
    .expect("alias fixture");
    let unsupported = std::process::Command::new("node")
        .arg("-e")
        .arg(FRONTMATTER_ID_SCRIPT)
        .current_dir(temp.path())
        .output()
        .expect("run frontmatter validator");
    assert!(!unsupported.status.success());
    assert!(
        String::from_utf8_lossy(&unsupported.stdout)
            .contains("unsupported-frontmatter-alias-or-tag")
    );

    let block_collection = skills.join("collection/SKILL.md");
    std::fs::write(
        block_collection,
        "---\nname: collection\ndescription:\n  - one\n---\nBody.\n",
    )
    .expect("block collection fixture");
    let invalid = std::process::Command::new("node")
        .arg("-e")
        .arg(FRONTMATTER_ID_SCRIPT)
        .current_dir(temp.path())
        .output()
        .expect("run frontmatter validator");
    assert!(!invalid.status.success());
    assert!(String::from_utf8_lossy(&invalid.stdout).contains("non-string-frontmatter-scalar"));
}

#[test]
fn strict_json_scope_rejects_missing_and_symlinked_files() {
    if std::process::Command::new("node")
        .arg("--version")
        .output()
        .is_err()
    {
        return;
    }
    let temp = tempfile::tempdir().expect("temporary repository");
    std::fs::write(temp.path().join("valid.json"), r#"{"ok":true}"#).expect("valid JSON");
    let run = |files: &[&str]| {
        std::process::Command::new("node")
            .arg("-e")
            .arg(STRICT_JSON_SCRIPT)
            .arg(serde_json::to_string(files).expect("encode file scope"))
            .current_dir(temp.path())
            .output()
            .expect("run strict JSON validator")
    };
    assert!(run(&["valid.json"]).status.success());
    assert!(!run(&["missing.json"]).status.success());

    std::fs::write(temp.path().join("out-of-scope.json"), "{ invalid").expect("invalid JSON");
    assert!(run(&["valid.json"]).status.success());
    std::fs::write(temp.path().join("duplicate.json"), r#"{"key":1,"key":2}"#)
        .expect("duplicate-key JSON");
    let duplicate = run(&["duplicate.json"]);
    assert!(!duplicate.status.success());
    assert!(String::from_utf8_lossy(&duplicate.stdout).contains("duplicate-key"));

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("valid.json", temp.path().join("file-link.json"))
            .expect("file symlink");
        std::fs::create_dir(temp.path().join("real-dir")).expect("real parent");
        std::fs::write(temp.path().join("real-dir/file.json"), r#"{"ok":true}"#)
            .expect("nested JSON");
        std::os::unix::fs::symlink("real-dir", temp.path().join("dir-link"))
            .expect("parent symlink");
        assert!(!run(&["file-link.json"]).status.success());
        assert!(!run(&["dir-link/file.json"]).status.success());
    }
}

#[test]
fn verify_vectors_pass_renderer_argv_gates() {
    let catalog = ToolCatalog::pinned();
    for kind in ValidatorKind::consumer_verify() {
        let Some(command) =
            verify_command(kind, &VerifyConfig::default(), &catalog).expect("command builds")
        else {
            continue;
        };
        velnor_actions_workflow_renderer::validate_command_argv(&command.argv)
            .expect("argv gates pass");
        if !command.prepare_argv.is_empty() {
            velnor_actions_workflow_renderer::validate_command_argv(&command.prepare_argv)
                .expect("prepare gates pass");
        }
        for arg in &command.argv {
            // Script args are whitespace-split by the lock audit; a
            // stray `mise` token inside one blocks generation.
            if arg.chars().any(char::is_whitespace) {
                assert!(
                    !arg.split_whitespace().any(|token| token == "mise"),
                    "no stray mise token trips the lock audit"
                );
            }
        }
    }
    for script in [STRICT_JSON_SCRIPT, FRONTMATTER_ID_SCRIPT] {
        assert!(!script.contains(['\n', '\r', '`']), "single line");
        assert!(!script.contains("$("), "no substitution");
        // No single quotes: the shellcheck pass over rendered `run:`
        // lines cannot parse quote escapes inside large quoted args.
        assert!(!script.contains('\''), "no single quotes");
    }
}

#[test]
fn verify_exec_pairs_with_matching_install() {
    let catalog = ToolCatalog::pinned();
    for kind in ValidatorKind::consumer_verify() {
        let Some(command) =
            verify_command(kind, &VerifyConfig::default(), &catalog).expect("command builds")
        else {
            continue;
        };
        let exec = command.argv.iter().find_map(|arg| {
            (command.argv.iter().any(|word| word == "exec") && arg.contains('@'))
                .then_some(arg.as_str())
        });
        match exec {
            None => assert!(
                command.prepare_argv.is_empty(),
                "{kind:?} carries no exec, so no install"
            ),
            Some(spec) => {
                assert_eq!(command.prepare_argv.last().map(String::as_str), Some(spec));
                assert!(
                    command.prepare_argv.iter().any(|word| word == "install"),
                    "{kind:?} prepares through install"
                );
                assert!(
                    super::validator_install_pin(spec).is_some(),
                    "{spec} resolves through the validator pin"
                );
            }
        }
    }
}

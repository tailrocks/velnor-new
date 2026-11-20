// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use std::time::{Duration, UNIX_EPOCH};

use crate::io::Repo;
use crate::io_scope_cache::prepared;
use velnor_test_support::trust_mise_dir;

fn run(repo: &Repo, operation: &str, specification: Option<&str>, cache: &Path) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    trust_mise_dir(&mut command, repo.root());
    command
        .current_dir(repo.root())
        .env("GITHUB_JOB", "job")
        .env_remove("VELNOR_BASE_MAX_AGE_DAYS")
        .env("GITHUB_OUTPUT", repo.root().join("outputs"));
    if operation == "restore-all" {
        command
            .args([
                "inputs",
                operation,
                "--roots-env",
                "INPUT_PATHS",
                "--scope-cache",
            ])
            .arg(cache);
    } else {
        command.args(["restored", "--targets-env", "INPUT_PATHS"]);
    }
    if let Some(specification) = specification {
        command.env("INPUT_PATHS", specification);
    } else {
        command.env_remove("INPUT_PATHS");
    }
    command
        .output()
        .unwrap_or_else(|error| unreachable!("{error}"))
}

#[test]
fn environment_path_specs_preserve_literal_expressions_quotes_and_newlines() {
    let (repo, cache) = prepared();
    let target = Path::new("cache-${{literal}}\n'space");
    let built = UNIX_EPOCH + Duration::from_secs(1_700_000_123);
    repo.set_mtime("src/lib.rs", built);
    velnor_ci::io::save_inputs(repo.root(), target).unwrap_or_else(|error| unreachable!("{error}"));
    repo.set_mtime("src/lib.rs", built + Duration::from_secs(5));
    let roots = r#"[["2e","63616368652d247b7b6c69746572616c7d7d0a277370616365"]]"#;
    let output = run(&repo, "restore-all", Some(roots), &cache);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(repo.mtime("src/lib.rs"), built);
    let targets = r#"["63616368652d247b7b6c69746572616c7d7d0a277370616365"]"#;
    let output = run(&repo, "restored", Some(targets), &cache);
    assert!(output.status.success(), "{output:?}");
    assert!(
        repo.root()
            .join(target)
            .join(".velnor/inputs.tsv")
            .is_file()
    );
    assert!(!repo.root().join("cache-\n'space").exists());
}

#[test]
fn invalid_environment_path_specs_change_no_targets_sources_or_existing_cache() {
    let (repo, cache) = prepared();
    repo.write_unstaged("cache/sentinel", "keep\n");
    let before = fs::read(&cache).unwrap_or_else(|error| unreachable!("{error}"));
    let source = fs::read(repo.root().join("src/lib.rs")).unwrap_or_default();
    let source_time = repo.mtime("src/lib.rs");
    for (operation, invalid) in [
        ("restore-all", None),
        ("restore-all", Some(r#"[["2e","6361636865"],["2e"]]"#)),
        (
            "restore-all",
            Some(r#"[["2e","6361636865"],[1,"6361636865"]]"#),
        ),
        ("restore-all", Some(r#"[["2e","6361636865"],["2e","zz"]]"#)),
        ("restore-all", Some("[]")),
        ("restored", None),
        ("restored", Some(r#"["6361636865",1]"#)),
        ("restored", Some(r#"["6361636865","zz"]"#)),
        ("restored", Some("[]")),
    ] {
        let output = run(&repo, operation, invalid, &cache);
        assert!(
            !output.status.success(),
            "{operation}: {invalid:?}: {output:?}"
        );
        assert_eq!(
            fs::read(repo.root().join("cache/sentinel")).unwrap_or_default(),
            b"keep\n"
        );
        assert_eq!(fs::read(&cache).unwrap_or_default(), before);
        assert_eq!(
            fs::read(repo.root().join("src/lib.rs")).unwrap_or_default(),
            source
        );
        assert_eq!(repo.mtime("src/lib.rs"), source_time);
        assert!(
            repo.root()
                .join("custom-target/.velnor/inputs.tsv")
                .is_file()
        );
    }
}

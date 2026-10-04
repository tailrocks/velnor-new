use super::{jsonc, parse_lock, source_candidates};

fn lock(resolution: &str, registry: &str) -> String {
    format!(
        r#"{{"lockfileVersion":1,"configVersion":1,"workspaces":{{"":{{}}}},"packages":{{"a":["{resolution}","{registry}",{{}},"sha512-{}=="],}},}}"#,
        "A".repeat(86)
    )
}

#[test]
fn real_text_lock_trailing_commas_preserve_exact_source_descriptor() {
    let sources = parse_lock(&lock("@scope/a@1.2.3", "")).expect("text lock");
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].name, "@scope/a");
    assert_eq!(
        sources[0].resolved,
        "https://registry.npmjs.org/@scope/a/-/a-1.2.3.tgz"
    );
}

#[test]
fn unsupported_sources_never_grant_candidates() {
    for source in [
        "git+https://private.example/repo",
        "a@file:../secret",
        "a@workspace:*",
    ] {
        assert!(parse_lock(&lock(source, "")).is_none());
    }
    for registry in [
        "https://private.example/a.tgz",
        "https://token@registry.npmjs.org/a/-/a-1.2.3.tgz",
    ] {
        assert!(parse_lock(&lock("a@1.2.3", registry)).is_none());
    }
    assert!(
        parse_lock(&lock("a@1.2.3", "").replace("\"configVersion\":1", "\"configVersion\":2"))
            .is_none()
    );
}

#[test]
fn comment_stripping_never_changes_quoted_urls_or_comma_strings() {
    assert_eq!(
        jsonc(r#"{"url":"https://x/a,}",/* comment */ "a":[1,],}"#).expect("JSONC"),
        r#"{"url":"https://x/a,}",  "a":[1]}"#
    );
    assert!(jsonc("{/* unterminated").is_none());
}

#[cfg(unix)]
#[test]
fn tracked_lock_symlink_never_reads_external_source_descriptors() {
    let root = tempfile::tempdir().expect("root");
    let outside = tempfile::tempdir().expect("outside");
    std::fs::write(root.path().join("package.json"), "{}").expect("manifest");
    std::fs::write(outside.path().join("lock"), lock("a@1.2.3", "")).expect("external lock");
    std::os::unix::fs::symlink(outside.path().join("lock"), root.path().join("bun.lock"))
        .expect("lock link");
    let index = velnor_actions_contract::build_index_from_list(
        root.path(),
        &["bun.lock".to_owned(), "package.json".to_owned()],
        &[],
    )
    .expect("tracked index");
    assert!(source_candidates(&index, ".").is_none());
}

#[test]
fn oversized_lock_is_rejected_by_bounded_reader() {
    let root = tempfile::tempdir().expect("root");
    std::fs::write(root.path().join("package.json"), "{}").expect("manifest");
    let file = std::fs::File::create(root.path().join("bun.lock")).expect("lock");
    file.set_len(4 * 1024 * 1024 + 1).expect("oversized lock");
    let index = velnor_actions_contract::build_index_from_list(
        root.path(),
        &["bun.lock".to_owned(), "package.json".to_owned()],
        &[],
    )
    .expect("tracked index");
    assert!(source_candidates(&index, ".").is_none());
}

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn derived_fixture(
    lock_name: &str,
    lock_contents: &str,
    refused_config: Option<&str>,
) -> Result<Vec<velnor_actions_contract::ProposedTask>, Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    std::fs::create_dir_all(root.path().join(".velnor"))?;
    std::fs::create_dir_all(root.path().join("ui"))?;
    std::fs::write(
        root.path().join(".velnor/config.toml"),
        "schema = 1\n[[stacks.workloads]]\nname = \"ui\"\nkind = \"bun_ci\"\nroot = \"ui\"\n",
    )?;
    std::fs::write(root.path().join("ui/package.json"), "{}")?;
    std::fs::write(root.path().join("ui").join(lock_name), lock_contents)?;
    if let Some(config) = refused_config {
        std::fs::write(root.path().join(config), "private-config")?;
    }
    let paths = vec!["ui/package.json".to_owned(), format!("ui/{lock_name}")];
    let index = velnor_actions_contract::build_index_from_list(root.path(), &paths, &[])?;
    let config = crate::config::load_config(root.path())?;
    Ok(crate::workloads::derive(&config, &index)?)
}

#[test]
fn bun_derivation_discovers_candidates_without_granting_execution_reuse() -> TestResult {
    let tasks = derived_fixture("bun.lock", &lock("a@1.2.3", ""), None)?;
    assert_eq!(tasks.len(), 3);
    let mut descriptor = None;
    for task in &tasks {
        let source = task
            .identity
            .environment
            .get("VELNOR_NATIVE_BUN_SOURCE_CANDIDATES")
            .ok_or("missing Bun candidate metadata")?;
        let sources: Vec<crate::workloads::cache_eligibility::NativeNpmSource> =
            serde_json::from_str(source)?;
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].name, "a");
        assert_eq!(sources[0].version, "1.2.3");
        assert_eq!(
            descriptor.get_or_insert_with(|| source.clone()).as_str(),
            source
        );
        assert!(
            !task
                .identity
                .environment
                .contains_key("VELNOR_NATIVE_NPM_SOURCE_CANDIDATES")
        );
        assert_eq!(task.reads, ["**"]);
        assert!(task.identity.undeclared_reads);
        assert!(!task.cache_policy.allow_task_reuse);
        assert!(!task.cache_policy.allow_compilation_reuse);
    }
    assert_eq!(
        tasks[0].payload,
        ["bun", "install", "--frozen-lockfile"].map(std::ffi::OsString::from)
    );
    Ok(())
}

#[test]
fn bun_derivation_refuses_config_and_unsupported_locks_without_losing_obligations() -> TestResult {
    let supported = lock("a@1.2.3", "");
    for (name, contents, config) in [
        ("bun.lock", supported.as_str(), Some(".npmrc")),
        ("bun.lock", supported.as_str(), Some("ui/bunfig.toml")),
        ("bun.lockb", "binary fixture", None),
        ("bun.lock", "unsupported fixture", None),
    ] {
        let tasks = derived_fixture(name, contents, config)?;
        assert_eq!(tasks.len(), 3);
        assert!(tasks.iter().all(|task| {
            !task
                .identity
                .environment
                .contains_key("VELNOR_NATIVE_BUN_SOURCE_CANDIDATES")
        }));
        assert_eq!(
            tasks[0].payload,
            ["bun", "install", "--frozen-lockfile"].map(std::ffi::OsString::from)
        );
    }
    Ok(())
}

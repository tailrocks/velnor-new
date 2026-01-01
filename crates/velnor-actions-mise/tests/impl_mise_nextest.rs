//! Fixed Nextest archive/list/run vector cases.
use std::ffi::OsString;
use velnor_actions_mise::{
    ARCHIVE_FILE, MiseError, NEXTEST_EXTRACT_BASE, NextestArchive, NextestDriver, NextestList,
    NextestPartition, NextestRun, PinnedTool, ToolCatalog,
};

fn pinned() -> ToolCatalog {
    ToolCatalog::pinned()
}

fn strings(items: &[&str]) -> Vec<OsString> {
    items.iter().map(OsString::from).collect()
}

#[test]
fn archive_argv_is_byte_exact_per_driver() -> Result<(), String> {
    assert_eq!(ARCHIVE_FILE, "target/nextest/tests.tar.zst");
    let mbx = NextestArchive::new(
        NextestDriver::Mbx,
        "demo",
        &["b".to_owned(), "a".to_owned()],
        Some("x86_64-unknown-linux-gnu"),
    )
    .map_err(|err| err.to_string())?;
    assert_eq!(
        mbx.argv(&pinned()),
        strings(&[
            "mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "exec",
            "rust@1.98.1",
            "mr-boxington@1.21.1",
            "aqua:nextest-rs/nextest/cargo-nextest@0.9.146",
            "--",
            "mbx",
            "nextest",
            "archive",
            "--package",
            "demo",
            "--profile",
            "ci",
            "--cargo-profile",
            "test",
            "--locked",
            "--archive-file",
            "target/nextest/tests.tar.zst",
            "--features",
            "a,b",
            "--target",
            "x86_64-unknown-linux-gnu",
        ])
    );
    let cargo = NextestArchive::new(NextestDriver::Cargo, "demo", &[], None)
        .map_err(|err| err.to_string())?;
    let argv = cargo.argv(&pinned());
    let split = argv.iter().position(|arg| arg == "--").expect("separator");
    assert_eq!(argv[split + 1], OsString::from("cargo"));
    assert!(
        argv.iter()
            .any(|arg| arg == "aqua:nextest-rs/nextest/cargo-nextest@0.9.146")
    );
    assert!(
        !argv.iter().any(|arg| arg == "--target"),
        "host builds omit --target: {argv:?}"
    );
    assert!(
        !argv.iter().any(|arg| arg == "--features"),
        "empty features emit no flag: {argv:?}"
    );
    Ok(())
}

#[test]
fn list_argv_is_byte_exact() -> Result<(), String> {
    let partition = NextestPartition::new(2, 4).map_err(|err| err.to_string())?;
    assert_eq!(partition.partition_arg(), "hash:2/4");
    let list = NextestList::new(NextestDriver::Mbx, partition);
    assert_eq!(
        list.argv(&pinned()),
        strings(&[
            "mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "exec",
            "rust@1.98.1",
            "mr-boxington@1.21.1",
            "aqua:nextest-rs/nextest/cargo-nextest@0.9.146",
            "--",
            "mbx",
            "nextest",
            "list",
            "--profile",
            "ci",
            "--archive-file",
            "target/nextest/tests.tar.zst",
            "--locked",
            "--message-format",
            "json",
            "--partition",
            "hash:2/4",
        ])
    );
    Ok(())
}

#[test]
fn run_argv_is_byte_exact_with_no_tests_fail() -> Result<(), String> {
    assert_eq!(NEXTEST_EXTRACT_BASE, "$RUNNER_TEMP/velnor/nextest");
    let partition = NextestPartition::new(1, 1).map_err(|err| err.to_string())?;
    let run = NextestRun::new(
        NextestDriver::Mbx,
        partition,
        "m-0123456789abcdef",
        "shard-1-of-1",
    )
    .map_err(|err| err.to_string())?;
    let argv = run.argv(&pinned());
    assert_eq!(
        argv,
        strings(&[
            "mise",
            "--no-config",
            "--no-env",
            "--no-hooks",
            "exec",
            "rust@1.98.1",
            "mr-boxington@1.21.1",
            "aqua:nextest-rs/nextest/cargo-nextest@0.9.146",
            "--",
            "mbx",
            "nextest",
            "run",
            "--profile",
            "ci",
            "--archive-file",
            "target/nextest/tests.tar.zst",
            "--extract-to",
            "$RUNNER_TEMP/velnor/nextest/m-0123456789abcdef/shard-1-of-1",
            "--locked",
            "--no-tests",
            "fail",
            "--partition",
            "hash:1/1",
        ])
    );
    Ok(())
}

#[test]
fn partitions_never_carry_compile_inputs() -> Result<(), String> {
    let partition = NextestPartition::new(1, 2).map_err(|err| err.to_string())?;
    for argv in [
        NextestList::new(NextestDriver::Cargo, partition).argv(&pinned()),
        NextestRun::new(NextestDriver::Cargo, partition, "m-abc", "p1")
            .map_err(|err| err.to_string())?
            .argv(&pinned()),
    ] {
        for token in ["build", "--package", "--cargo-profile", "--features"] {
            assert!(
                !argv.iter().any(|arg| arg == token),
                "partitions must not recompile ({token}): {argv:?}"
            );
        }
    }
    Ok(())
}

#[test]
fn driver_selects_program_and_tools() {
    assert_eq!(NextestDriver::Cargo.program(), "cargo");
    assert_eq!(NextestDriver::Mbx.program(), "mbx");
    assert_eq!(
        NextestDriver::Cargo.tools(),
        vec![PinnedTool::Rust, PinnedTool::Nextest]
    );
    assert_eq!(
        NextestDriver::Mbx.tools(),
        vec![
            PinnedTool::Rust,
            PinnedTool::MrBoxington,
            PinnedTool::Nextest
        ]
    );
}

#[test]
fn malformed_inputs_are_rejected() {
    assert!(matches!(
        NextestPartition::new(1, 0),
        Err(MiseError::InvalidNextestInput { .. })
    ));
    assert!(matches!(
        NextestPartition::new(3, 2),
        Err(MiseError::InvalidNextestInput { .. })
    ));
    assert!(NextestArchive::new(NextestDriver::Cargo, "", &[], None).is_err());
    assert!(NextestArchive::new(NextestDriver::Cargo, "a/b", &[], None).is_err());
    assert!(NextestArchive::new(NextestDriver::Cargo, "demo", &["a b".to_owned()], None).is_err());
    assert!(matches!(
        NextestArchive::new(NextestDriver::Cargo, "demo", &["a b".to_owned()], None),
        Err(MiseError::InvalidNextestInput { field, .. }) if field == "feature"
    ));
    assert!(NextestArchive::new(NextestDriver::Cargo, "demo", &[], Some("")).is_err());
    let partition = NextestPartition::new(1, 1).expect("partition");
    assert!(NextestRun::new(NextestDriver::Cargo, partition, "../escape", "p1").is_err());
    assert!(NextestRun::new(NextestDriver::Cargo, partition, "m-abc", "a/b").is_err());
    assert!(NextestRun::new(NextestDriver::Cargo, partition, "", "p1").is_err());
}

#[test]
fn nextest_archive_accepts_config_feature_syntax() -> Result<(), String> {
    // M1: the config gate accepts weak-dependency feature syntax; the archive
    // must use the same contract charset instead of the narrower token set.
    let archive = NextestArchive::new(
        NextestDriver::Cargo,
        "demo",
        &[
            "serde?/derive".to_owned(),
            "tokio:rt".to_owned(),
            "dep/feat".to_owned(),
        ],
        None,
    )
    .map_err(|err| err.to_string())?;
    assert_eq!(archive.config_key().2.len(), 3);
    Ok(())
}

#[test]
fn nextest_commands_match_argv() -> Result<(), String> {
    let archive = NextestArchive::new(NextestDriver::Cargo, "demo", &[], None)
        .map_err(|err| err.to_string())?;
    assert_eq!(
        archive
            .command(&pinned())
            .map_err(|err| err.to_string())?
            .argv(),
        archive.argv(&pinned())
    );
    let partition = NextestPartition::new(1, 1).map_err(|err| err.to_string())?;
    let list = NextestList::new(NextestDriver::Cargo, partition);
    assert_eq!(
        list.command(&pinned())
            .map_err(|err| err.to_string())?
            .argv(),
        list.argv(&pinned())
    );
    let run = NextestRun::new(NextestDriver::Cargo, partition, "m-abc", "p1")
        .map_err(|err| err.to_string())?;
    assert_eq!(
        run.command(&pinned())
            .map_err(|err| err.to_string())?
            .argv(),
        run.argv(&pinned())
    );
    Ok(())
}

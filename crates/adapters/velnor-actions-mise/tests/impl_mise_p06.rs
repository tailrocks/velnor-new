//! P06 detection cases: structural wrappers, Nextest config, profile argv.
use velnor_actions_mise::toolfiles::{MbxWrapperStatus, mbx_wrapper_pin_status};
use velnor_actions_mise::{
    CI_PROFILE_NAME, DEFAULT_PROFILE_NAME, MR_BOXINGTON_VERSION, NEXTEST_CONFIG_REL,
    NextestArchive, NextestDriver, NextestList, NextestPartition, NextestRun, PinnedTool,
    ToolCatalog, is_mbx_command, parse_cargo_wrapper, parse_nextest_config,
};

/// This repository's own `mise.toml` wrapper line (inline-table spelling).
const REPO_WRAPPER_LINE: &str =
    "wrappers = { cargo = { command = \"mbx\", env = { MBX_CARGO_SHIM_MODE = \"1\" } } }";

/// This repository's own `.config/nextest.toml` bytes.
const REPO_NEXTEST: &str = "# Nextest profiles (Gate 0 skeleton).\n# Empty-run behavior is enforced via CLI `--no-tests fail` (contract §4);\n# nextest has no profile-level equivalent, so it is not set here.\n[profile.ci]\nretries = 0\ntest-threads = \"num-cpus\"\nstatus-level = \"all\"\nfinal-status-level = \"flaky\"\n";

#[test]
fn wrapper_inline_table_form() {
    let wrapper = parse_cargo_wrapper(REPO_WRAPPER_LINE)
        .expect("repo wrapper parses")
        .expect("repo wrapper found");
    assert_eq!(wrapper.command, "mbx");
    assert_eq!(wrapper.line, 1);
    assert_eq!(wrapper.shim_mode.as_deref(), Some("1"));
    assert!(wrapper.is_mbx());
}

#[test]
fn wrapper_section_forms() {
    for (content, line) in [
        ("[wrappers.cargo]\ncommand = \"mbx\"\n", 2),
        ("[wrappers]\ncargo.command = \"mbx\"\n", 2),
        ("wrappers.cargo.command = \"mbx\"\n", 1),
        ("[wrappers.cargo]\ncommand = 'mbx'\n", 2),
    ] {
        let wrapper = parse_cargo_wrapper(content)
            .expect("section form parses")
            .expect("section form found");
        assert_eq!(wrapper.command, "mbx", "{content:?}");
        assert_eq!(wrapper.line, line, "{content:?}");
        assert_eq!(wrapper.shim_mode, None, "{content:?}");
        assert!(wrapper.is_mbx());
    }
}

#[test]
fn wrapper_shim_section_form() {
    let content =
        "[wrappers.cargo]\ncommand = \"mbx\"\n[wrappers.cargo.env]\nMBX_CARGO_SHIM_MODE = \"1\"\n";
    let wrapper = parse_cargo_wrapper(content)
        .expect("shim form parses")
        .expect("shim form found");
    assert!(wrapper.is_mbx());
    assert_eq!(wrapper.shim_mode.as_deref(), Some("1"));
}

#[test]
fn wrapper_comment_is_not_evidence() {
    for content in [
        "# wrappers.cargo.command = \"mbx\"\n",
        "#wrappers = { cargo = { command = \"mbx\" } }\n",
        "[wrappers.cargo]\n# command = \"mbx\"\n",
        "[tools]\n# mr_boxington leftovers\nrust = \"1.98.1\"\n",
    ] {
        assert_eq!(parse_cargo_wrapper(content), Ok(None), "{content:?}");
    }
}

#[test]
fn wrapper_string_mention_is_not_evidence() {
    for content in [
        "[tools]\nnote = \"wrappers.cargo.command = mbx\"\n",
        "[wrappers.cargo]\nnote = \"command mbx everywhere\"\n",
        "[wrappers]\nblurb = 'cargo command \"mbx\"'\n",
        "[wrappers.other]\ncommand = \"mbx\"\n",
    ] {
        assert_eq!(parse_cargo_wrapper(content), Ok(None), "{content:?}");
    }
}

#[test]
fn wrapper_near_miss_commands_not_mbx() {
    for command in ["not-mbx", "mbx2", "my mbx", "MBX", "/usr/bin/mbx", ""] {
        assert!(!is_mbx_command(command), "{command:?} must not match");
        let content = format!("[wrappers.cargo]\ncommand = \"{command}\"\n");
        let wrapper = parse_cargo_wrapper(&content)
            .expect("near miss parses")
            .expect("near miss found");
        assert!(!wrapper.is_mbx(), "{command:?}");
    }
    assert!(is_mbx_command("mbx"));
}

#[test]
fn wrapper_absent_is_none() {
    for content in ["", "\n", "[tools]\nrust = \"1.98.1\"\n", "[wrappers]\n"] {
        assert_eq!(parse_cargo_wrapper(content), Ok(None), "{content:?}");
    }
}

#[test]
fn wrapper_malformed_is_diagnostic() {
    let err = parse_cargo_wrapper("[wrappers\ncommand = \n").expect_err("must fail");
    assert_eq!(err.line, 1);
    assert!(!err.problem.is_empty());
    assert!(err.to_string().contains("wrapper_invalid"));
    let err = parse_cargo_wrapper("[wrappers.cargo]\ncommand = \"mbx\n")
        .expect_err("unterminated string fails");
    assert_eq!(err.line, 2);
    let err = parse_cargo_wrapper("[wrappers.cargo]\ncommand = \"a\"\ncommand = \"b\"\n")
        .expect_err("duplicate fails");
    assert_eq!(err.problem, "duplicate_key");
}

#[test]
fn wrapper_wrong_types_rejected() {
    let err =
        parse_cargo_wrapper("[wrappers.cargo]\ncommand = true\n").expect_err("bool command fails");
    assert_eq!((err.line, err.problem.as_str()), (2, "command_not_string"));
    let err = parse_cargo_wrapper(
        "[wrappers.cargo]\ncommand = \"mbx\"\n[wrappers.cargo.env]\nMBX_CARGO_SHIM_MODE = 1\n",
    )
    .expect_err("numeric shim fails");
    assert_eq!(
        (err.line, err.problem.as_str()),
        (4, "shim_mode_not_string")
    );
}

#[test]
fn nextest_ci_profile_selected() {
    assert_eq!(NEXTEST_CONFIG_REL, ".config/nextest.toml");
    let config = parse_nextest_config(REPO_NEXTEST).expect("repo nextest parses");
    assert_eq!(config.profiles, vec!["ci".to_owned()]);
    assert_eq!(config.ci_line, Some(4));
    assert!(config.has_ci_profile());
    assert_eq!(config.selected_profile(), CI_PROFILE_NAME);
    assert_eq!(config.selected_profile(), "ci");
}

#[test]
fn nextest_default_without_ci() {
    let config =
        parse_nextest_config("[profile.linux]\nretries = 1\n").expect("other profile parses");
    assert_eq!(config.profiles, vec!["linux".to_owned()]);
    assert_eq!(config.ci_line, None);
    assert!(!config.has_ci_profile());
    assert_eq!(config.selected_profile(), DEFAULT_PROFILE_NAME);
    assert_eq!(config.selected_profile(), "default");
}

#[test]
fn nextest_ci_subsection_and_dotted_forms() {
    for content in [
        "[profile.ci.junit]\npath = \"junit.xml\"\n",
        "profile.ci.retries = 0\n",
        "[profile]\nci.retries = 0\n",
    ] {
        let config = parse_nextest_config(content).expect("ci form parses");
        assert!(config.has_ci_profile(), "{content:?}");
        assert_eq!(config.selected_profile(), "ci");
    }
}

#[test]
fn nextest_comment_is_not_evidence() {
    let config =
        parse_nextest_config("# [profile.ci]\n# retries = 0\n").expect("comment-only parses");
    assert!(config.profiles.is_empty());
    assert_eq!(config.selected_profile(), "default");
}

#[test]
fn nextest_empty_and_malformed() {
    let config = parse_nextest_config("").expect("empty parses");
    assert!(config.profiles.is_empty());
    assert_eq!(config.selected_profile(), "default");
    let err = parse_nextest_config("[profile.ci\nretries = \n").expect_err("must fail");
    assert_eq!(err.line, 1);
    assert!(err.to_string().contains("nextest_config_invalid"));
}

#[test]
fn nextest_four_combos_argv() {
    let catalog = ToolCatalog::pinned();
    let rust = catalog.tool_spec(PinnedTool::Rust);
    let mbx = catalog.tool_spec(PinnedTool::MrBoxington);
    let nextest = catalog.tool_spec(PinnedTool::Nextest);
    assert!(rust.starts_with("rust@"), "{rust}");
    assert!(mbx.starts_with("mr-boxington@"), "{mbx}");
    assert!(nextest.contains("nextest@"), "{nextest}");
    for driver in [NextestDriver::Cargo, NextestDriver::Mbx] {
        let program = driver.program();
        for profile in ["ci", "default"] {
            let archive =
                NextestArchive::with_profile(driver, "demo", &[], None, profile).expect("archive");
            assert_eq!(archive.profile(), profile);
            let payload = argv_text(&archive.payload());
            assert_eq!(payload[0], program, "{driver:?}/{profile}");
            assert!(payload.windows(2).any(|w| w == ["--profile", profile]));
            let full = argv_text(&archive.argv(&catalog));
            assert_eq!(full[0], "mise");
            assert!(full.contains(&rust), "{driver:?}/{profile} rust");
            assert!(full.contains(&nextest), "{driver:?}/{profile} nextest");
            assert!(
                !full.contains(&mbx),
                "{driver:?}/{profile} leaves action-owned MBX out of Mise selectors"
            );

            let partition = NextestPartition::new(1, 1).expect("partition");
            let list = NextestList::with_profile(driver, partition, profile).expect("list");
            let payload = argv_text(&list.payload());
            assert_eq!(payload[0], program);
            assert!(payload.windows(2).any(|w| w == ["--profile", profile]));

            let run =
                NextestRun::with_profile(driver, partition, "m-abc", "p1", profile).expect("run");
            let payload = argv_text(&run.payload());
            assert_eq!(payload[0], program);
            assert!(payload.windows(2).any(|w| w == ["--profile", profile]));
            let full = argv_text(&run.argv(&catalog));
            assert!(full.contains(&nextest));
            assert!(
                !full.contains(&mbx),
                "{driver:?}/{profile} leaves action-owned MBX out of Mise selectors"
            );
        }
    }
}

#[test]
fn nextest_default_constructors_match_ci() {
    let archive = NextestArchive::new(NextestDriver::Mbx, "demo", &[], None).expect("archive");
    let explicit =
        NextestArchive::with_profile(NextestDriver::Mbx, "demo", &[], None, "ci").expect("ci");
    assert_eq!(archive.payload(), explicit.payload());
    assert_eq!(archive.profile(), "ci");
    let partition = NextestPartition::new(1, 2).expect("partition");
    let list = NextestList::new(NextestDriver::Cargo, partition);
    let explicit =
        NextestList::with_profile(NextestDriver::Cargo, partition, "ci").expect("ci list");
    assert_eq!(list.payload(), explicit.payload());
    let run = NextestRun::new(NextestDriver::Cargo, partition, "m-abc", "p1").expect("run");
    let explicit = NextestRun::with_profile(NextestDriver::Cargo, partition, "m-abc", "p1", "ci")
        .expect("ci run");
    assert_eq!(run.payload(), explicit.payload());
}

#[test]
fn consumed_config_selects_emitted_profile() {
    let catalog = ToolCatalog::pinned();
    let nextest = catalog.tool_spec(PinnedTool::Nextest);
    let driver = NextestDriver::Mbx;
    for (content, want) in [
        (REPO_NEXTEST, "ci"),
        ("[profile.linux]\nretries = 1\n", "default"),
    ] {
        let config = parse_nextest_config(content).expect("config");
        let selected = config.selected_profile();
        assert_eq!(selected, want);
        let partition = NextestPartition::new(1, 1).expect("partition");
        let archive =
            NextestArchive::with_profile(driver, "demo", &[], None, selected).expect("archive");
        assert!(carries_profile(&archive.payload(), want), "{want}");
        let full = argv_text(&archive.argv(&catalog));
        assert!(full.contains(&nextest), "{want}");
        let list = NextestList::with_profile(driver, partition, selected).expect("list");
        assert!(carries_profile(&list.payload(), want), "{want}");
        let run =
            NextestRun::with_profile(driver, partition, "m-abc", "p1", selected).expect("run");
        assert!(carries_profile(&run.payload(), want), "{want}");
    }
}

#[test]
fn resolved_default_differs_from_baked_ci() {
    let baked = NextestArchive::new(NextestDriver::Cargo, "demo", &[], None).expect("archive");
    assert_eq!(baked.profile(), "ci");
    let resolved = NextestArchive::with_profile(NextestDriver::Cargo, "demo", &[], None, "default")
        .expect("archive");
    assert_ne!(baked.payload(), resolved.payload());
    assert!(carries_profile(&resolved.payload(), "default"));
}

#[test]
fn nextest_bad_profile_rejected() {
    for profile in ["", "has space", "semi;colon", "quote\"x"] {
        assert!(
            NextestArchive::with_profile(NextestDriver::Cargo, "demo", &[], None, profile).is_err(),
            "{profile:?}"
        );
        let partition = NextestPartition::new(1, 1).expect("partition");
        assert!(NextestList::with_profile(NextestDriver::Cargo, partition, profile).is_err());
        assert!(
            NextestRun::with_profile(NextestDriver::Cargo, partition, "m", "p", profile).is_err()
        );
    }
}

/// Lossy text of one argv vector.
fn argv_text(argv: &[std::ffi::OsString]) -> Vec<String> {
    argv.iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect()
}

/// True when argv carries `--profile <want>`.
fn carries_profile(argv: &[std::ffi::OsString], want: &str) -> bool {
    argv_text(argv).windows(2).any(|w| w == ["--profile", want])
}

/// This repository's own root `mise.toml` bytes (P07-9 reconciliation).
const REPO_MISE_TOML: &str = include_str!("../../../../mise.toml");

#[test]
fn repo_wrapper_reconciles_with_local_mbx_pin() {
    let status = mbx_wrapper_pin_status(REPO_MISE_TOML);
    assert_eq!(
        status,
        MbxWrapperStatus::Pinned {
            version: MR_BOXINGTON_VERSION.to_owned(),
        },
        "root wrapper must resolve MBX from the local exact pin"
    );
    let catalog = ToolCatalog::pinned();
    let spec = catalog.tool_spec(PinnedTool::MrBoxington);
    assert_eq!(spec, format!("mr-boxington@{MR_BOXINGTON_VERSION}"));
    let install = velnor_actions_mise::MiseInstall::new(vec![PinnedTool::MrBoxington])
        .expect("mbx install request");
    let argv = argv_text(&install.argv(&catalog));
    assert_eq!(argv[0], "mise");
    assert!(
        argv.contains(&"--no-config".to_owned()),
        "isolated install never loads config: {argv:?}"
    );
    assert!(argv.contains(&"--no-env".to_owned()), "clean-env: {argv:?}");
    assert!(
        argv.contains(&"--no-hooks".to_owned()),
        "clean-env: {argv:?}"
    );
    assert!(argv.contains(&"install".to_owned()), "{argv:?}");
    assert!(
        argv.contains(&spec),
        "fresh checkout installs MBX from the pin: {argv:?}"
    );
}

#[test]
fn mbx_wrapper_without_pin_is_missing() {
    for content in [
        "[wrappers.cargo]\ncommand = \"mbx\"\n[tools]\nrust = \"1.98.1\"\n",
        "[wrappers.cargo]\ncommand = \"mbx\"\n",
    ] {
        assert_eq!(
            mbx_wrapper_pin_status(content),
            MbxWrapperStatus::MissingPin,
            "{content:?}"
        );
    }
}

#[test]
fn mbx_wrapper_loose_pin_is_invalid() {
    for selector in ["latest", "v1.19.0", "1.19", "1"] {
        let content = format!(
            "[wrappers.cargo]\ncommand = \"mbx\"\n[tools]\nmr-boxington = \"{selector}\"\n"
        );
        assert!(
            matches!(
                mbx_wrapper_pin_status(&content),
                MbxWrapperStatus::Invalid { .. }
            ),
            "{selector} must fail closed"
        );
    }
    assert!(matches!(
        mbx_wrapper_pin_status("[wrappers.cargo]\ncommand = \"mbx\"\n[tools]\nbroken = \n"),
        MbxWrapperStatus::Invalid { .. }
    ));
}

#[test]
fn non_mbx_wrapper_needs_no_pin() {
    for content in [
        "[tools]\nrust = \"1.98.1\"\n",
        "[wrappers.cargo]\ncommand = \"cargo\"\n[tools]\nrust = \"1.98.1\"\n",
    ] {
        assert_eq!(
            mbx_wrapper_pin_status(content),
            MbxWrapperStatus::NoMbxWrapper,
            "{content:?}"
        );
    }
}

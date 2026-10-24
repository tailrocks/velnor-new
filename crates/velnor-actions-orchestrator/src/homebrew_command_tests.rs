use super::*;

#[test]
fn commands_have_closed_program_argv_and_environment() {
    let home = Path::new("/private/runner/home");
    let temp = Path::new("/private/runner/tmp");
    let install = homebrew_command(
        HomebrewCommand::InstallCandidate {
            recipe: PathBuf::from("/private/runner/staging/example-preview.rb"),
        },
        home,
        temp,
    )
    .expect("fixed install command");
    assert_eq!(
        install.argv(),
        [
            OsString::from(HOMEBREW_EXECUTABLE),
            OsString::from("install"),
            OsString::from("--formula"),
            OsString::from("/private/runner/staging/example-preview.rb"),
        ]
    );
    let upgrade = homebrew_command(
        HomebrewCommand::UpgradeCandidate {
            recipe: PathBuf::from("/private/runner/staging/example-preview-d.rb"),
        },
        home,
        temp,
    )
    .expect("fixed upgrade command");
    assert_eq!(
        upgrade.argv(),
        [
            OsString::from(HOMEBREW_EXECUTABLE),
            OsString::from("upgrade"),
            OsString::from("--formula"),
            OsString::from("--no-ask"),
            OsString::from("/private/runner/staging/example-preview-d.rb"),
        ]
    );
    let test = homebrew_command(
        HomebrewCommand::TestArchived {
            recipe: PathBuf::from(
                "/opt/homebrew/Cellar/example-preview/1.2.3/.brew/example-preview.rb",
            ),
        },
        home,
        temp,
    )
    .expect("archived recipe test command");
    assert_eq!(
        test.argv(),
        [
            OsString::from(HOMEBREW_EXECUTABLE),
            OsString::from("test"),
            OsString::from("--force"),
            OsString::from("/opt/homebrew/Cellar/example-preview/1.2.3/.brew/example-preview.rb"),
        ]
    );
    assert!(!test.argv().iter().any(|argument| argument == "--formula"));
    let debug = format!("{test:?}");
    for setting in [
        "HOMEBREW_NO_AUTO_UPDATE",
        "HOMEBREW_NO_INSTALL_CLEANUP",
        "HOMEBREW_NO_ANALYTICS",
        "HOMEBREW_NO_INSTALL_FROM_API",
        "HOMEBREW_GIT_PATH",
        "HOMEBREW_FORCE_VENDOR_RUBY",
    ] {
        assert!(debug.contains(setting));
    }
    assert!(debug.contains("/private/runner/home"));
    assert!(debug.contains("/private/runner/tmp"));
    assert!(debug.contains("HOMEBREW_FORCE_VENDOR_RUBY"));
    assert!(debug.contains("HOMEBREW_GIT_PATH"));
}

#[test]
fn homebrew_config_command_is_a_fixed_read_only_query() {
    let command = homebrew_command(
        HomebrewCommand::Config,
        Path::new("/private/runner/home"),
        Path::new("/private/runner/tmp"),
    )
    .expect("fixed config command");
    assert_eq!(
        command.argv(),
        [
            OsString::from(HOMEBREW_EXECUTABLE),
            OsString::from("config")
        ]
    );
    let debug = format!("{command:?}");
    assert!(debug.contains("HOMEBREW_FORCE_VENDOR_RUBY"));
    assert!(debug.contains('1'));
}

#[test]
fn portable_ruby_install_is_the_first_fixed_archive_bootstrap_request() {
    let command = homebrew_command(
        HomebrewCommand::InstallPortableRuby,
        Path::new("/private/runner/home"),
        Path::new("/private/runner/tmp"),
    )
    .expect("fixed portable Ruby installer");
    assert_eq!(
        command.argv(),
        [
            OsString::from(HOMEBREW_EXECUTABLE),
            OsString::from("vendor-install"),
            OsString::from("ruby"),
        ]
    );
    let debug = format!("{command:?}");
    assert!(debug.contains("HOMEBREW_NO_SUDO"));
    assert!(debug.contains("HOMEBREW_CURL_PATH"));
    assert!(!debug.contains("GITHUB_TOKEN"));
}

#[test]
fn prefix_moves_have_only_the_closed_sudo_move_argv() {
    let home = Path::new("/private/runner/home");
    let temp = Path::new("/private/runner/tmp");
    let runner_temp = Path::new("/private/runner");
    let source = PathBuf::from("/private/runner/velnor-homebrew-stage-0123");
    validate_prefix_move(&source, Path::new("/opt/homebrew"), runner_temp)
        .expect("staging activation path is allowed");
    let command = isolated_move_command(
        vec![
            OsString::from("-n"),
            OsString::from("/bin/mv"),
            source.clone().into_os_string(),
            OsString::from("/opt/homebrew"),
        ],
        home,
        temp,
    )
    .expect("fixed prefix activation argv");
    assert_eq!(
        command.argv(),
        [
            OsString::from("/usr/bin/sudo"),
            OsString::from("-n"),
            OsString::from("/bin/mv"),
            source.into_os_string(),
            OsString::from("/opt/homebrew"),
        ]
    );
    for (source, destination) in [
        ("/tmp/user", "/opt/homebrew"),
        ("/opt/homebrew", "/tmp/user"),
        ("/opt/homebrew", "/opt/homebrew/inside"),
    ] {
        assert!(
            validate_prefix_move(Path::new(source), Path::new(destination), runner_temp).is_err()
        );
    }
}

#[test]
fn command_paths_reject_relative_traversal_and_non_formula_files() {
    let home = Path::new("/private/home");
    let temp = Path::new("/private/tmp");
    assert!(
        homebrew_command(
            HomebrewCommand::InstallCandidate {
                recipe: PathBuf::from("relative/formula.rb"),
            },
            home,
            temp
        )
        .is_err()
    );
    assert!(
        homebrew_command(
            HomebrewCommand::UpgradeCandidate {
                recipe: PathBuf::from("relative/formula.rb"),
            },
            home,
            temp
        )
        .is_err()
    );
    assert!(
        homebrew_command(
            HomebrewCommand::TestArchived {
                recipe: PathBuf::from("/tmp/../formula.rb"),
            },
            home,
            temp
        )
        .is_err()
    );
    assert!(
        homebrew_command(
            HomebrewCommand::InstallCandidate {
                recipe: PathBuf::from("/tmp/formula.txt"),
            },
            home,
            temp
        )
        .is_err()
    );
    assert!(homebrew_command(HomebrewCommand::Version, Path::new("relative-home"), temp).is_err());
}

const HOMEBREW_EXECUTABLE: &str = "/opt/homebrew/bin/brew";

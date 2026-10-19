//! Component probes retain exact compiler, platform, and banner authority.
use super::*;
use crate::config::{CheckPlatform, QualifiedToolArtifactRecipe};

const BANNER: &str = "rustfmt 1.10.0-stable (58cf8f9c35 2026-10-13)";
const COMPILER: &str = "rustc 1.99.1 (58cf8f9c3 2026-10-13)\nbinary: rustc\ncommit-hash: 58cf8f9c35b355199727627d07ae34a4e2846d67\ncommit-date: 2026-10-13\nhost: aarch64-apple-darwin\nrelease: 1.99.1\nLLVM version: 23.1.1";

fn recipe() -> QualifiedToolRecipe {
    QualifiedToolRecipe {
        id: "rust".to_owned(),
        backend: QualifiedToolBackend::Core { tool: "rust".to_owned() },
        version: "1.99.1".to_owned(),
        options: QualifiedToolOptions::Rust {
            components: vec!["rustfmt".to_owned()], targets: Vec::new(),
        },
        depends_on: Vec::new(),
        platforms: vec![QualifiedToolRecipePlatform {
            platform: CheckPlatform::MacosArm64,
            artifacts: vec![QualifiedToolArtifactRecipe {
                url: "https://static.rust-lang.org/dist/2026-10-15/rustfmt-1.99.1-aarch64-apple-darwin.tar.xz".to_owned(),
            }],
            dependency_artifacts: Vec::new(),
            executables: vec![
                QualifiedToolExecutableRecipe {
                    name: "cargo-fmt".to_owned(), path: "bin/cargo-fmt".to_owned(),
                    probe: QualifiedToolProbe::RustfmtVersion { expected: BANNER.to_owned() },
                },
                QualifiedToolExecutableRecipe {
                    name: "rustc".to_owned(), path: "bin/rustc".to_owned(),
                    probe: QualifiedToolProbe::RustcVerbose { expected: COMPILER.to_owned() },
                },
                QualifiedToolExecutableRecipe {
                    name: "rustfmt".to_owned(), path: "bin/rustfmt".to_owned(),
                    probe: QualifiedToolProbe::RustfmtVersion { expected: BANNER.to_owned() },
                },
            ],
        }],
    }
}

fn rejected(recipe: &QualifiedToolRecipe) {
    assert!(recipe.validate("test", "tool").is_err(), "{recipe:?}");
}

fn compiler_output(recipe: &mut QualifiedToolRecipe, output: String) {
    recipe.platforms[0].executables[1].probe =
        QualifiedToolProbe::RustcVerbose { expected: output };
}

#[test]
fn rustfmt_component_admits_both_canonical_executables_and_roundtrips() {
    let valid = recipe();
    valid
        .validate("test", "tool")
        .expect("official formatter and compiler identities");
    let json = serde_json::to_value(&valid).expect("serialize recipe");
    assert_eq!(
        json["platforms"][0]["executables"][0]["probe"]["kind"],
        "rustfmt_version"
    );
    assert_eq!(
        serde_json::from_value::<QualifiedToolRecipe>(json).expect("roundtrip"),
        valid
    );
    let leap = COMPILER.replace("2026-10-13", "2024-02-29");
    let mut valid = recipe();
    compiler_output(&mut valid, leap);
    for index in [0, 2] {
        valid.platforms[0].executables[index].probe = QualifiedToolProbe::RustfmtVersion {
            expected: BANNER.replace("2026-10-13", "2024-02-29"),
        };
    }
    valid.validate("test", "tool").expect("real leap date");
}

#[test]
fn rustfmt_probe_rejects_other_backends_components_names_paths_and_shapes() {
    let mut invalid = recipe();
    invalid.backend = QualifiedToolBackend::Aqua {
        package: "owner/tool".to_owned(),
    };
    rejected(&invalid);
    for component in ["", "rustfmt-preview", "clippy"] {
        let mut invalid = recipe();
        invalid.options = QualifiedToolOptions::Rust {
            components: if component.is_empty() {
                Vec::new()
            } else {
                vec![component.to_owned()]
            },
            targets: Vec::new(),
        };
        rejected(&invalid);
    }
    for (name, path) in [
        ("cargo-fmt", "cargo-fmt"),
        ("cargo-fmt", "bin/rustfmt"),
        ("fmt", "bin/fmt"),
    ] {
        let mut invalid = recipe();
        invalid.platforms[0].executables[0].name = name.to_owned();
        invalid.platforms[0].executables[0].path = path.to_owned();
        rejected(&invalid);
    }
    let mut invalid = recipe();
    invalid.platforms[0].executables[0].probe = QualifiedToolProbe::Version {
        expected: BANNER.to_owned(),
    };
    rejected(&invalid);
}

#[test]
fn rustfmt_banner_rejects_forged_versions_suffixes_hashes_dates_and_controls() {
    for banner in [
        "",
        "rustfmt 01.10.0-stable (58cf8f9c35 2026-10-13)",
        "rustfmt 1.10-stable (58cf8f9c35 2026-10-13)",
        "rustfmt 1.10.0-beta (58cf8f9c35 2026-10-13)",
        "rustfmt 1.10.0-stable (58CF8F9C35 2026-10-13)",
        "rustfmt 1.10.0-stable (58cf8f9c3 2026-10-13)",
        "rustfmt 1.10.0-stable (58cf8f9c350 2026-10-13)",
        "rustfmt 1.10.0-stable (18cf8f9c35 2026-10-13)",
        "rustfmt 1.10.0-stable (58cf8f9c35 2026-02-30)",
        "rustfmt 1.10.0-stable (58cf8f9c35 2026-10-14)",
        "rustfmt 1.10.0-stable (58cf8f9c35 0000-10-13)",
        "rustfmt 1.10.0-stable (58cf8f9c35 2026-10-13)\n",
        "rustfmt 1.10.0-stable (58cf8f9c35 2026-10-13)\r\n",
        "rustfmt 1.10.0-stable (58cf8f9c35 2026-10-13) ",
        "rustfmt 1.10.0-stable (58cf8f9c35 2026-10-13) extra",
        "rustfmt １.10.0-stable (58cf8f9c35 2026-10-13)",
    ] {
        let mut invalid = recipe();
        invalid.platforms[0].executables[0].probe = QualifiedToolProbe::RustfmtVersion {
            expected: banner.to_owned(),
        };
        rejected(&invalid);
    }
}

#[test]
fn rustfmt_requires_unique_same_platform_canonical_compiler_identity() {
    for output in [
        COMPILER.replace("release: 1.99.1", "release: 1.99.0"),
        COMPILER.replace("host: aarch64-apple-darwin", "host: x86_64-apple-darwin"),
        COMPILER.replace("commit-hash: 58cf", "commit-hash: 58CF"),
        COMPILER.replace("58cf8f9c35b355199727627d07ae34a4e2846d67", "58cf8f9c35"),
        COMPILER.replace("commit-date: 2026-10-13", "commit-date: 2026-02-30"),
        COMPILER.replace("commit-date: 2026-10-13\n", ""),
        COMPILER.replace(
            "commit-hash: 58cf8f9c35b355199727627d07ae34a4e2846d67\n",
            "",
        ),
        format!("{COMPILER}\ncommit-date: 2026-10-13"),
        format!("{COMPILER}\ncommit-hash: 58cf8f9c35b355199727627d07ae34a4e2846d67"),
        format!("{COMPILER}\ncommit-hash: 0000000000000000000000000000000000000000"),
        format!("{COMPILER}\nrelease: 1.99.1"),
        format!("{COMPILER}\nhost: aarch64-apple-darwin"),
    ] {
        let mut invalid = recipe();
        compiler_output(&mut invalid, output);
        rejected(&invalid);
    }
    for (name, path) in [("compiler", "bin/rustc"), ("rustc", "rustc")] {
        let mut invalid = recipe();
        invalid.platforms[0].executables[1].name = name.to_owned();
        invalid.platforms[0].executables[1].path = path.to_owned();
        rejected(&invalid);
    }
    let mut invalid = recipe();
    invalid.platforms[0].executables.remove(1);
    rejected(&invalid);
    let mut invalid = recipe();
    invalid.platforms[0].executables[1].probe = QualifiedToolProbe::Version {
        expected: "rustc 1.99.1".to_owned(),
    };
    rejected(&invalid);
    let mut invalid = recipe();
    invalid.platforms[0].platform = CheckPlatform::MacosX64;
    invalid.platforms[0].artifacts[0].url = invalid.platforms[0].artifacts[0]
        .url
        .replace("aarch64-apple-darwin", "x86_64-apple-darwin");
    rejected(&invalid);
}

#[test]
fn rustfmt_cannot_borrow_compiler_from_another_platform_or_duplicate_identity() {
    let valid = recipe();
    let mut platform = valid.platforms[0].clone();
    platform.executables.remove(1);
    assert!(validate(&valid, &platform, &platform.executables[0], "test", "tool").is_err());
    let mut platform = valid.platforms[0].clone();
    platform.executables.push(platform.executables[1].clone());
    assert!(validate(&valid, &platform, &platform.executables[0], "test", "tool").is_err());
    let mut invalid = recipe();
    invalid.platforms[0].executables[0].probe = QualifiedToolProbe::Version {
        expected: BANNER.to_owned(),
    };
    rejected(&invalid);
}

#[test]
fn rustfmt_admission_checks_backend_and_host_independently_of_source_urls() {
    let mut invalid = recipe();
    invalid.backend = QualifiedToolBackend::Aqua {
        package: "owner/tool".to_owned(),
    };
    let platform = &invalid.platforms[0];
    assert!(validate(&invalid, platform, &platform.executables[0], "test", "tool").is_err());
    let valid = recipe();
    let mut platform = valid.platforms[0].clone();
    platform.platform = CheckPlatform::MacosX64;
    assert!(validate(&valid, &platform, &platform.executables[0], "test", "tool").is_err());
}

#[test]
fn rustfmt_probe_serde_rejects_unknown_fields_kinds_and_arbitrary_arguments() {
    for json in [
        r#"{"kind":"rustfmt_version","expected":"x","argv":["--version"]}"#,
        r#"{"kind":"rustfmt_version","expected":"x","command":"rustfmt"}"#,
        r#"{"kind":"rustfmt","expected":"x"}"#,
    ] {
        assert!(
            serde_json::from_str::<QualifiedToolProbe>(json).is_err(),
            "{json}"
        );
    }
}

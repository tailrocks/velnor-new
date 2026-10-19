//! Formatter component invocation and complete executable-proof regression tests.

use super::*;

const FORMATTER: &str = "rustfmt 1.10.0-stable (58cf8f9c35 2026-10-13)";
const COMMIT: &str = "58cf8f9c35b355199727627d07ae34a4e2846d67";

fn compiler_output(platform: CheckPlatform) -> String {
    format!(
        "rustc 1.99.1 (58cf8f9c3 2026-10-13)\nbinary: rustc\ncommit-hash: {COMMIT}\ncommit-date: 2026-10-13\nhost: {}\nrelease: 1.99.1",
        platform.target()
    )
}

fn formatter_tool(platform: CheckPlatform) -> QualifiedTool {
    let executables = ["cargo-fmt", "rustc", "rustfmt"]
        .into_iter()
        .map(|name| QualifiedToolExecutable {
            name: name.into(),
            path: format!("bin/{name}"),
            sha256: "c".repeat(64),
            probe: if name == "rustc" {
                QualifiedToolProbe::RustcVerbose {
                    expected: compiler_output(platform),
                }
            } else {
                QualifiedToolProbe::RustfmtVersion {
                    expected: FORMATTER.into(),
                }
            },
        })
        .collect();
    QualifiedTool {
        id: "rust".into(),
        version: "1.99.1".into(),
        backend: QualifiedToolBackend::Core {
            tool: "rust".into(),
        },
        options: QualifiedToolOptions::Rust {
            components: vec!["rustfmt".into()],
            targets: vec![],
        },
        depends_on: vec![],
        platforms: vec![QualifiedToolPlatform {
            platform,
            artifacts: vec![QualifiedToolArtifact {
                url: format!(
                    "https://static.rust-lang.org/dist/rust-1.99.1-{}.tar.xz",
                    platform.target()
                ),
                sha256: "a".repeat(64),
            }],
            dependency_artifacts: vec![],
            install_tree_sha256: "b".repeat(64),
            executables,
        }],
    }
}

#[test]
fn rustfmt_probe_accepts_only_exact_banner_and_optional_final_newline() -> TestResult {
    let fixture = Fixture::new()?;
    let platform = platform()?;
    let tool = formatter_tool(platform);
    let declared = &tool.platforms[0].executables[2];
    for suffix in ["", "\\n", "\\r\\n"] {
        let observed = fixture.observation(
            "rustfmt",
            &format!("test \"$#\" = 1; test \"$1\" = --version; printf '%s{suffix}' '{FORMATTER}'"),
        )?;
        verify_qualified_executable(
            &tool,
            platform,
            declared,
            &observed,
            &fixture.homes,
            test_deadline()?,
        )?;
    }
    Ok(())
}

#[test]
fn rustfmt_probe_rejects_trailing_altered_or_failed_output() -> TestResult {
    let fixture = Fixture::new()?;
    let platform = platform()?;
    let tool = formatter_tool(platform);
    let declared = &tool.platforms[0].executables[2];
    for script in [
        format!("printf '%s\\nextra\\n' '{FORMATTER}'"),
        format!("printf '%s\\n\\n' '{FORMATTER}'"),
        format!("printf '%s\\r' '{FORMATTER}'"),
        format!(
            "printf '%s\\n' '{}';",
            FORMATTER.replace("1.10.0", "1.10.1")
        ),
        format!("printf '%s\\n' '{FORMATTER}'; exit 1"),
    ] {
        let observed = fixture.observation("rustfmt", &script)?;
        assert!(
            verify_qualified_executable(
                &tool,
                platform,
                declared,
                &observed,
                &fixture.homes,
                test_deadline()?,
            )
            .is_err()
        );
    }
    Ok(())
}

#[test]
fn formatter_receipts_require_complete_ordered_unchanged_proofs() -> TestResult {
    let fixture = Fixture::new()?;
    let platform = platform()?;
    let tool = formatter_tool(platform);
    let mut proofs = Vec::new();
    for declared in &tool.platforms[0].executables {
        let expected = if declared.name == "rustc" {
            compiler_output(platform)
        } else {
            FORMATTER.into()
        };
        let observed =
            fixture.observation(&declared.name, &format!("printf '%s\\n' '{expected}'"))?;
        proofs.push(verify_qualified_executable(
            &tool,
            platform,
            declared,
            &observed,
            &fixture.homes,
            test_deadline()?,
        )?);
    }
    validate_executable_proofs(&tool, platform, &proofs)?;
    assert!(validate_executable_proofs(&tool, platform, &proofs[..2]).is_err());
    let mut duplicate = proofs.clone();
    duplicate[1] = duplicate[0].clone();
    assert!(validate_executable_proofs(&tool, platform, &duplicate).is_err());
    let mut altered = proofs.clone();
    altered[0].stdout.push_str("extra");
    altered[0].stdout_digest = velnor_actions_contract::digest_b3(altered[0].stdout.as_bytes());
    assert!(validate_executable_proofs(&tool, platform, &altered).is_err());
    altered = proofs.clone();
    altered[2].observed.sha256 = "d".repeat(64);
    assert!(validate_executable_proofs(&tool, platform, &altered).is_err());
    Ok(())
}

#[test]
fn cargo_fmt_probe_does_not_require_rustfmt_projected_earlier() -> TestResult {
    let fixture = Fixture::new()?;
    let platform = platform()?;
    let tool = formatter_tool(platform);
    let declared = &tool.platforms[0].executables[0];
    assert_eq!(declared.name, "cargo-fmt");
    assert!(!fixture.homes.bin_dir.join("rustfmt").exists());
    let observed = fixture.observation(
        "cargo-fmt",
        &format!("test \"$#\" = 1; test \"$1\" = --version; test ! -e \"$HOME/bin/rustfmt\"; test \"$RUSTUP_TOOLCHAIN\" = \"$HOME/compiler\"; printf '%s\\n' '{FORMATTER}'"),
    )?;
    verify_qualified_executable(
        &tool,
        platform,
        declared,
        &observed,
        &fixture.homes,
        test_deadline()?,
    )?;
    Ok(())
}

#[test]
fn rustfmt_probe_revalidates_compiler_binding_before_execution() -> TestResult {
    let fixture = Fixture::new()?;
    let platform = platform()?;
    let mut tool = formatter_tool(platform);
    let QualifiedToolProbe::RustcVerbose { expected } = &mut tool.platforms[0].executables[1].probe
    else {
        return Err("fixture compiler probe changed".into());
    };
    *expected = expected.replace("2026-10-13", "2026-10-14");
    let observed = fixture.observation(
        "rustfmt",
        &format!("touch \"$HOME/EXECUTED\"; printf '%s\\n' '{FORMATTER}'"),
    )?;
    assert!(
        verify_qualified_executable(
            &tool,
            platform,
            &tool.platforms[0].executables[2],
            &observed,
            &fixture.homes,
            test_deadline()?,
        )
        .is_err()
    );
    assert!(!fixture.root.join("EXECUTED").exists());
    Ok(())
}

use super::*;

const BANNER: &str = "ShellCheck - shell script analysis tool\nversion: 0.11.0\nlicense: GNU General Public License, version 3\nwebsite: https://www.shellcheck.net";

fn shellcheck(probe: QualifiedToolProbe, platform: CheckPlatform) -> QualifiedTool {
    let mut tool = super::tool("shellcheck", "0.11.0", probe, platform);
    tool.id = "shellcheck-ci".into();
    tool.backend = QualifiedToolBackend::Aqua {
        package: "koalaman/shellcheck".into(),
    };
    tool.platforms[0].artifacts[0].url =
        "https://github.com/koalaman/shellcheck/releases/download/v0.11.0/shellcheck-v0.11.0.tar.gz"
            .into();
    tool
}

#[test]
fn ordinary_version_cannot_validate_shellchecks_second_line_version() -> TestResult {
    let fixture = Fixture::new()?;
    let platform = platform()?;
    let tool = shellcheck(
        QualifiedToolProbe::Version {
            expected: "0.11.0".into(),
        },
        platform,
    );
    let observed = fixture.observation(
        "shellcheck",
        &format!("test \"$1\" = --version; printf '%s\\n' '{BANNER}'"),
    )?;

    assert!(tool.validate("config", "qualified_tools[0]").is_ok());
    assert!(
        verify_qualified_executable(
            &tool,
            platform,
            &tool.platforms[0].executables[0],
            &observed,
            &fixture.homes,
            test_deadline()?,
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn shellcheck_probe_uses_fixed_argv_and_accepts_only_one_optional_final_lf() -> TestResult {
    for final_lf in [false, true] {
        let fixture = Fixture::new()?;
        let platform = platform()?;
        let tool = shellcheck(
            QualifiedToolProbe::ShellCheckVersion {
                expected: BANNER.into(),
            },
            platform,
        );
        let ending = if final_lf { "\n" } else { "" };
        let script = format!(
            "test \"$#\" = 1; test \"$1\" = --version; test \"${{GITHUB_TOKEN-unset}}\" = unset; test \"${{RUSTUP_TOOLCHAIN-unset}}\" = unset; test \"${{MISE_CONFIG_FILE-unset}}\" = unset; test \"$CARGO_HOME\" = \"$HOME/cargo\"; printf '%s' '{BANNER}{ending}'"
        );
        let observed = fixture.observation("shellcheck", &script)?;
        let proof = verify_qualified_executable(
            &tool,
            platform,
            &tool.platforms[0].executables[0],
            &observed,
            &fixture.homes,
            test_deadline()?,
        )?;
        assert_eq!(
            proof.stdout,
            format!("{BANNER}{}", if final_lf { "\n" } else { "" })
        );
        validate_executable_proofs(&tool, platform, &[proof])?;
    }
    Ok(())
}

#[test]
fn shellcheck_probe_rejects_extra_output_wrong_status_and_foreign_bytes() -> TestResult {
    let scripts = [
        format!("test \"$1\" = --version; printf '%s\\n%s' '{BANNER}' extra"),
        format!("test \"$1\" = --version; printf '%s\\n\\n' '{BANNER}'"),
        format!(
            "test \"$1\" = --version; printf '%s' '{}'",
            BANNER.replace("version: 0.11.0", "version: 0.11.1")
        ),
        format!("test \"$1\" = --version; printf '%s' '{BANNER}' >&2"),
        format!("test \"$1\" = version; printf '%s' '{BANNER}'"),
        format!("test \"$1\" = --version; printf '%s' '{BANNER}'; exit 1"),
        "test \"$1\" = --version; printf '\\377'".into(),
        "test \"$1\" = --version; printf '\\001'".into(),
    ];
    for script in scripts {
        let fixture = Fixture::new()?;
        let platform = platform()?;
        let tool = shellcheck(
            QualifiedToolProbe::ShellCheckVersion {
                expected: BANNER.into(),
            },
            platform,
        );
        let observed = fixture.observation("shellcheck", &script)?;
        assert!(
            verify_qualified_executable(
                &tool,
                platform,
                &tool.platforms[0].executables[0],
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
fn shellcheck_proof_revalidation_rejects_rehashed_output_and_declaration_drift() -> TestResult {
    let fixture = Fixture::new()?;
    let platform = platform()?;
    let tool = shellcheck(
        QualifiedToolProbe::ShellCheckVersion {
            expected: BANNER.into(),
        },
        platform,
    );
    let observed = fixture.observation(
        "shellcheck",
        &format!("test \"$1\" = --version; printf '%s\\n' '{BANNER}'"),
    )?;
    let proof = verify_qualified_executable(
        &tool,
        platform,
        &tool.platforms[0].executables[0],
        &observed,
        &fixture.homes,
        test_deadline()?,
    )?;
    validate_executable_proofs(&tool, platform, std::slice::from_ref(&proof))?;

    let mut altered_output = proof.clone();
    altered_output.stdout.push_str("\nextra");
    altered_output.stdout_digest =
        velnor_actions_contract::digest_b3(altered_output.stdout.as_bytes());
    assert!(validate_executable_proofs(&tool, platform, &[altered_output]).is_err());

    let mut altered_declaration = proof;
    altered_declaration.declared.probe = QualifiedToolProbe::ShellCheckVersion {
        expected: format!("{BANNER}\n"),
    };
    assert!(validate_executable_proofs(&tool, platform, &[altered_declaration]).is_err());
    Ok(())
}

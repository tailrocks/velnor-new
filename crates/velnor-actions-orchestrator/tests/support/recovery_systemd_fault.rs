// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[derive(Clone, Copy)]
struct FaultFixture<'a> {
    execution: &'a crate::crate_release_source_view::SupervisedSourceExecution,
    intent: &'a velnor_actions_contract::config::CrateReleaseIntent,
    git: &'a crate::crate_release_git::ReleaseGit,
    request: &'a GitHubAdmissionRequest,
    receipt: &'a crate::crate_release_controller::CrateReleaseAdmissionReceipt,
    registry: &'a Registry,
    original: &'a Path,
    prior: &'a crate::crate_release_journal::JournalReceipt,
    current: &'a crate::crate_release_journal::JournalReceipt,
    history_api: &'a super::HistoricalAuthority<'a>,
    reader: &'a crate::crate_release_workspace::HistoricalIntentReader<'a>,
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn check_supervised_fault(input: FaultFixture<'_>) -> Result<bool, Box<dyn std::error::Error>> {
    use crate::CrateSourceExecution as _;
    let FaultFixture {
        execution,
        intent,
        git,
        request,
        receipt,
        registry,
        original,
        history_api,
        ..
    } = input;
    if let Ok(fault) = std::env::var("VELNOR_TEST_CUSTODY_FAULT") {
        let journal = super::JournalRegistryFake::default();
        induce_supervised_fault(&fault, input)?;
        let generator = crate::package_release_signer::fixture_attested_generator();
        let attempted = crate::package_release_signer::AdmittedStart {
            journal: &journal,
            github: history_api,
            registry,
            intent,
            request,
            admission: receipt,
            source: execution.root(),
            git,
            source_execution: execution,
            workflow_root: original,
            generator: &generator,
        };
        assert!(
            crate::package_release_signer::record_admitted_start(&attempted).is_err(),
            "journal start accepted poisoned custody"
        );
        assert_eq!(
            journal.write_calls.get(),
            0,
            "wrote journal after custody fault"
        );
        assert!(
            execution.historical_intent_reader().is_err(),
            "poisoned session issued a history capability"
        );
        assert!(
            execution.metadata(execution.root(), intent, git).is_err(),
            "poisoned session resumed"
        );
        return Ok(true);
    }
    Ok(false)
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn induce_supervised_fault(
    fault: &str,
    input: FaultFixture<'_>,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::CrateSourceExecution as _;
    use std::os::unix::fs::PermissionsExt as _;
    let FaultFixture {
        execution,
        intent,
        git,
        request,
        prior,
        current,
        history_api,
        reader,
        ..
    } = input;
    match fault {
        "source" => {
            let source = execution.root();
            let moved = source.with_extension("displaced");
            fs::rename(source, &moved)?;
            fs::create_dir(source)?;
            fs::set_permissions(source, fs::Permissions::from_mode(0o555))?;
            assert!(
                execution
                    .verify_toolchain_separation(source, intent, git)
                    .is_err(),
                "source substitution accepted"
            );
            fs::remove_dir(source)?;
            fs::rename(moved, source)?;
        }
        "tool" => {
            let view = execution.root().parent().ok_or("source view")?;
            let cargo = view.join(format!(
                "rustup/toolchains/{}-x86_64-unknown-linux-gnu/bin/cargo",
                intent.rust_version
            ));
            let moved = cargo.with_extension("displaced");
            fs::rename(&cargo, &moved)?;
            fs::write(&cargo, b"substituted")?;
            fs::set_permissions(&cargo, fs::Permissions::from_mode(0o555))?;
            assert!(
                execution
                    .verify_toolchain_separation(execution.root(), intent, git)
                    .is_err(),
                "tool substitution accepted"
            );
            fs::remove_file(&cargo)?;
            fs::rename(moved, cargo)?;
        }
        "history" => {
            let mut forged = prior.clone();
            forged.intent_sha256 = "e".repeat(64);
            assert!(
                reader
                    .verify_transition(&forged, current, request, history_api)
                    .is_err(),
                "forged prior intent accepted"
            );
        }
        _ => return Err("unknown fixture fault".into()),
    }
    Ok(())
}

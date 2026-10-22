use super::*;

fn shellcheck() -> QualifiedTool {
    let version = "0.11.0";
    let expected = format!(
        "ShellCheck - shell script analysis tool\nversion: {version}\nlicense: GNU General Public License, version 3\nwebsite: https://www.shellcheck.net"
    );
    QualifiedTool {
        id: "shellcheck-ci".into(),
        backend: velnor_actions_contract::config::QualifiedToolBackend::Aqua {
            package: "koalaman/shellcheck".into(),
        },
        version: version.into(),
        options: velnor_actions_contract::config::QualifiedToolOptions::Default,
        depends_on: Vec::new(),
        platforms: vec![velnor_actions_contract::config::QualifiedToolPlatform {
            platform: CheckPlatform::LinuxX64,
            artifacts: vec![velnor_actions_contract::config::QualifiedToolArtifact {
                url: format!(
                    "https://github.com/koalaman/shellcheck/releases/download/v{version}/shellcheck.tar.xz"
                ),
                sha256: "a".repeat(64),
            }],
            dependency_artifacts: Vec::new(),
            install_tree_sha256: "b".repeat(64),
            executables: vec![velnor_actions_contract::config::QualifiedToolExecutable {
                name: "shellcheck".into(),
                path: "bin/shellcheck".into(),
                sha256: "c".repeat(64),
                probe: velnor_actions_contract::config::QualifiedToolProbe::ShellCheckVersion {
                    expected,
                },
            }],
        }],
    }
}

#[test]
fn named_check_keeps_the_closed_shellcheck_declaration_and_fingerprint() {
    let declaration = shellcheck();
    let mut check = super::check(
        "shellcheck",
        super::hosted("ubuntu-26.04", CheckPlatform::LinuxX64),
    );
    check.tools = vec![declaration.id.clone()];
    let (_root, discovery) =
        super::fixture_with_tools(&[check], std::slice::from_ref(&declaration));
    let row = &discovery.mise_checks[0];
    assert_eq!(row.qualified_tools, [declaration]);
    row.verify_qualification_identity()
        .expect("closed probe identity");
    let serialized = serde_json::to_value(&row.qualified_tools).expect("tool declaration");
    assert_eq!(
        serialized[0]["platforms"][0]["executables"][0]["probe"]["kind"],
        "shellcheck_version"
    );
    assert!(
        row.proposal
            .identity
            .flags
            .contains(&format!("qualified_tools:{}", row.qualification_digest))
    );

    let jobs = super::build_check_jobs(
        WorkflowPolicy::VelnorRepositoryV1,
        &discovery,
        &ToolCatalog::pinned(),
    )
    .expect("named check job");
    assert_eq!(jobs.len(), 1);
    assert!(
        jobs[0]
            .1
            .steps
            .iter()
            .any(|step| step.name == "Execute named check")
    );
}

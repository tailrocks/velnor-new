use super::*;

fn shellcheck(version: &str) -> QualifiedTool {
    let banner = format!(
        "ShellCheck - shell script analysis tool\nversion: {version}\nlicense: GNU General Public License, version 3\nwebsite: https://www.shellcheck.net"
    );
    let platform = CheckPlatform::LinuxX64;
    QualifiedTool {
        id: "shellcheck-ci".into(),
        backend: QualifiedToolBackend::Aqua {
            package: "koalaman/shellcheck".into(),
        },
        version: version.into(),
        options: QualifiedToolOptions::Default,
        depends_on: Vec::new(),
        platforms: vec![QualifiedToolPlatform {
            platform,
            artifacts: vec![QualifiedToolArtifact {
                url: format!(
                    "https://github.com/koalaman/shellcheck/releases/download/v{version}/shellcheck.tar.xz"
                ),
                sha256: "a".repeat(64),
            }],
            dependency_artifacts: Vec::new(),
            install_tree_sha256: "b".repeat(64),
            executables: vec![QualifiedToolExecutable {
                name: "shellcheck".into(),
                path: "bin/shellcheck".into(),
                sha256: "c".repeat(64),
                probe: QualifiedToolProbe::ShellCheckVersion { expected: banner },
            }],
        }],
    }
}

#[test]
fn shellcheck_banner_and_probe_kind_enter_the_qualification_fingerprint() {
    let tool = shellcheck("0.11.0");
    let serialized = serde_json::to_value(&tool).expect("serialize qualified tool");
    let probe = &serialized["platforms"][0]["executables"][0]["probe"];
    assert_eq!(probe["kind"], "shellcheck_version");
    assert!(
        probe["expected"]
            .as_str()
            .is_some_and(|value| value.contains("version: 0.11.0"))
    );
    let first = super::super::super::DiscoveredCheck::qualification_fingerprint(
        std::slice::from_ref(&tool),
        &["aqua:koalaman/shellcheck@0.11.0".into()],
    )
    .expect("valid closed probe fingerprint");

    let updated = shellcheck("0.11.1");
    let second = super::super::super::DiscoveredCheck::qualification_fingerprint(
        &[updated],
        &["aqua:koalaman/shellcheck@0.11.1".into()],
    )
    .expect("updated closed probe fingerprint");
    assert_ne!(first, second);

    let mut tampered = tool;
    let QualifiedToolProbe::ShellCheckVersion { expected } =
        &mut tampered.platforms[0].executables[0].probe
    else {
        panic!("ShellCheck probe");
    };
    expected.push_str("\nextra");
    assert!(
        super::super::super::DiscoveredCheck::qualification_fingerprint(
            &[tampered],
            &["aqua:koalaman/shellcheck@0.11.0".into()],
        )
        .is_err()
    );
}

use super::*;

const VERSION: &str = "0.11.0";

fn banner(version: &str) -> String {
    format!(
        "ShellCheck - shell script analysis tool\nversion: {version}\nlicense: GNU General Public License, version 3\nwebsite: https://www.shellcheck.net"
    )
}

fn shellcheck(probe: QualifiedToolProbe) -> QualifiedTool {
    QualifiedTool {
        id: "shellcheck-ci".into(),
        backend: QualifiedToolBackend::Aqua {
            package: "koalaman/shellcheck".into(),
        },
        version: VERSION.into(),
        options: QualifiedToolOptions::Default,
        depends_on: Vec::new(),
        platforms: vec![QualifiedToolPlatform {
            platform: CheckPlatform::LinuxX64,
            artifacts: vec![QualifiedToolArtifact {
                url: "https://github.com/koalaman/shellcheck/releases/download/v0.11.0/shellcheck-v0.11.0.linux.x86_64.tar.xz".into(),
                sha256: "a".repeat(64),
            }],
            dependency_artifacts: Vec::new(),
            install_tree_sha256: "b".repeat(64),
            executables: vec![QualifiedToolExecutable {
                name: "shellcheck".into(),
                path: "shellcheck-v0.11.0/shellcheck".into(),
                sha256: "c".repeat(64),
                probe,
            }],
        }],
    }
}

#[test]
fn shellcheck_probe_has_one_closed_serialized_kind() {
    let probe = QualifiedToolProbe::ShellCheckVersion {
        expected: banner(VERSION),
    };
    let mut value = serde_json::to_value(&probe).expect("serialize probe");
    assert_eq!(value["kind"], "shellcheck_version");
    assert_eq!(
        serde_json::from_value::<QualifiedToolProbe>(value.clone()).expect("decode probe"),
        probe
    );
    value["kind"] = "shell_check_version".into();
    assert!(serde_json::from_value::<QualifiedToolProbe>(value).is_err());
}

#[test]
fn canonical_banner_is_admitted_only_for_shellcheck_aqua_identity() {
    let tool = shellcheck(QualifiedToolProbe::ShellCheckVersion {
        expected: banner(VERSION),
    });
    assert!(tool.validate("config", "qualified_tools[0]").is_ok());
    assert!(!tool.requires_compiler());

    let mut old_probe = tool.clone();
    old_probe.platforms[0].executables[0].probe = QualifiedToolProbe::Version {
        expected: VERSION.into(),
    };
    assert!(old_probe.validate("config", "qualified_tools[0]").is_ok());

    let mut wrong_package = tool.clone();
    wrong_package.backend = QualifiedToolBackend::Aqua {
        package: "other/shellcheck".into(),
    };
    wrong_package.platforms[0].artifacts[0].url =
        "https://github.com/other/shellcheck/releases/download/v0.11.0/shellcheck.tar.xz".into();
    assert!(
        wrong_package
            .validate("config", "qualified_tools[0]")
            .is_err()
    );

    let mut foreign_backend = tool.clone();
    foreign_backend.backend = QualifiedToolBackend::Core { tool: "bun".into() };
    foreign_backend.platforms[0].artifacts[0].url =
        "https://github.com/oven-sh/bun/releases/download/bun-v0.11.0/bun-linux-x64.zip".into();
    assert!(
        foreign_backend
            .validate("config", "qualified_tools[0]")
            .is_err()
    );

    let mut wrong_executable = tool.clone();
    wrong_executable.platforms[0].executables[0].name = "sc".into();
    assert!(
        wrong_executable
            .validate("config", "qualified_tools[0]")
            .is_err()
    );
}

#[test]
fn shellcheck_expected_output_is_the_exact_version_bound_banner() {
    let expected = banner(VERSION);
    for output in [
        "ShellCheck - shell script analysis tool".to_owned(),
        "version: 0.11.0".to_owned(),
        expected.replace("version: 0.11.0", "version: 0.11.1"),
        expected.replace(
            "license: GNU General Public License, version 3",
            "license: other",
        ),
        expected.replace(
            "website: https://www.shellcheck.net",
            "website: https://example.com",
        ),
        expected.replace("version: 0.11.0\n", ""),
        format!("{expected}\nextra output"),
        format!("{expected}\nversion: 0.11.0"),
        format!("{expected}\n"),
        expected.replace('\n', "\r\n"),
    ] {
        let tool = shellcheck(QualifiedToolProbe::ShellCheckVersion { expected: output });
        assert!(tool.validate("config", "qualified_tools[0]").is_err());
    }

    let oversized = format!("{}{}", banner(VERSION), "x".repeat(9 * 1024));
    assert!(
        shellcheck(QualifiedToolProbe::ShellCheckVersion {
            expected: oversized
        })
        .validate("config", "qualified_tools[0]")
        .is_err()
    );

    let mut wrong_version = shellcheck(QualifiedToolProbe::ShellCheckVersion {
        expected: banner(VERSION),
    });
    wrong_version.version = "0.11.1".into();
    wrong_version.platforms[0].artifacts[0].url = wrong_version.platforms[0].artifacts[0]
        .url
        .replace("v0.11.0", "v0.11.1");
    assert!(
        wrong_version
            .validate("config", "qualified_tools[0]")
            .is_err()
    );
}

#[test]
fn unknown_probe_fields_remain_rejected() {
    let mut value = serde_json::to_value(QualifiedToolProbe::ShellCheckVersion {
        expected: banner(VERSION),
    })
    .expect("serialize probe");
    value["argv"] = serde_json::json!(["--version"]);
    assert!(serde_json::from_value::<QualifiedToolProbe>(value).is_err());
}

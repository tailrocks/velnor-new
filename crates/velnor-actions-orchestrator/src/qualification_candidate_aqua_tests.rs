//! Offline generic Aqua authoring safety and runtime parity fixtures.
use super::*;
use velnor_actions_contract::config::QualifiedToolBackend;

const SHELLCHECK_BANNER: &str = "ShellCheck - shell script analysis tool\nversion: {version}\nlicense: GNU General Public License, version 3\nwebsite: https://www.shellcheck.net";

fn expected_output(package: &str, version: &str, executable: &str) -> String {
    if package == "koalaman/shellcheck" {
        SHELLCHECK_BANNER.replace("{version}", version)
    } else {
        format!("{executable} {version}")
    }
}

fn aqua_fixture(
    package: &str,
    version: &str,
    executable: &str,
    program: &[u8],
) -> (tempfile::TempDir, Value) {
    let expected = expected_output(package, version, executable);
    let temp = tempfile::tempdir().expect("temp");
    let path = temp.path().join("tool.tar.gz");
    let executable_path = if package == "rust-cross/cargo-zigbuild" {
        executable.to_owned()
    } else {
        format!("bin/{executable}")
    };
    let entry = format!("release/{executable_path}");
    archive(
        &path,
        &[
            (&entry, program, 0o755),
            ("release/support", b"data", 0o644),
        ],
    );
    let hash = acquisition::sha256_with_deadline(&fs::read(&path).expect("bytes"), deadline())
        .expect("hash");
    let repository = package.split('/').take(2).collect::<Vec<_>>().join("/");
    let url = format!("https://github.com/{repository}/releases/download/v{version}/tool.tar.gz");
    let probe = if package == "koalaman/shellcheck" {
        json!({"kind":"shellcheck_version","expected":expected})
    } else {
        json!({"kind":"version","expected":expected})
    };
    let request = json!({"schema_version":1,"platform":native(),"timeout_seconds":60,"tools":[{
        "recipe":{"id":if package == "rust-cross/cargo-zigbuild" {"zigbuild"} else {package.rsplit('/').next().expect("tool id")},
            "backend":{"kind":"aqua","package":package},"version":version,
            "options":{"kind":"default"},"depends_on":[],
            "platforms":[{"platform":native(),"artifacts":[{"url":url}],
                "dependency_artifacts":[],"executables":[{"name":executable,"path":executable_path,
                    "probe":probe}]}]},
        "archives":[{"path":path,"url":url,"sha256":hash}]}]});
    (temp, request)
}

fn jactionlint_fixture(program: &[u8]) -> (tempfile::TempDir, Value) {
    aqua_fixture("jdx/jactionlint", "2.3.0", "jactionlint", program)
}

fn set_aqua_url(request: &mut Value, url: &str) {
    request["tools"][0]["recipe"]["platforms"][0]["artifacts"][0]["url"] = json!(url);
    request["tools"][0]["archives"][0]["url"] = json!(url);
}

fn replace(request: &mut Value, field: &str, value: Value) {
    *request.pointer_mut(field).expect("fixture field") = value;
}

fn changed(request: &Value, field: &str, value: Value) -> Value {
    let mut changed = request.clone();
    replace(&mut changed, field, value);
    changed
}

fn fixture_url(tag: &str) -> String {
    format!("https://github.com/velnor-fixture/fixture-tool/releases/download/{tag}/tool.tar.gz")
}

fn with_aqua_url(request: &Value, url: &str) -> Value {
    let mut changed = request.clone();
    set_aqua_url(&mut changed, url);
    changed
}

fn assert_rejected(temp: &Path, request: &Value) {
    assert!(run(temp, request).is_err());
    assert!(!temp.join("candidate.json").exists());
}

#[test]
fn safe_aqua_packages_share_candidate_and_runtime_validation() {
    // These scripts are synthetic fixture bytes, not observations of official releases.
    for (package, version, executable) in [
        ("jdx/jactionlint", "2.3.0", "jactionlint"),
        ("koalaman/shellcheck", "0.11.0", "shellcheck"),
        ("jdx/mr-boxington", "1.27.0", "mbx"),
        (
            "velnor-fixture/fixture-tool/fixture-tool",
            "7.8.9",
            "fixture-tool",
        ),
    ] {
        let expected = expected_output(package, version, executable);
        let stdout = if package == "koalaman/shellcheck" {
            expected.clone()
        } else {
            format!("{expected}\n")
        };
        let program =
            format!("#!/bin/sh\ntest \"$1\" = --version || exit 64\nprintf '%s' '{stdout}'\n");
        let (temp, request) = aqua_fixture(package, version, executable, program.as_bytes());
        let output = run(temp.path(), &request).expect("candidate");
        let value: Value =
            serde_json::from_slice(&fs::read(output).expect("output")).expect("json");
        assert_eq!(value["evidence_kind"], "qualified_tool_authoring_candidate");
        assert_eq!(value["probes"][0]["stdout"], stdout);
        let tools: Vec<QualifiedTool> =
            serde_json::from_value(value["qualified_tools"].clone()).expect("tools");
        assert_eq!(
            tools[0].backend,
            QualifiedToolBackend::Aqua {
                package: package.to_owned()
            }
        );
        validate_qualified_tools(&tools, "candidate").expect("complete runtime pins");
        acquisition::validate_installations(
            &tools,
            native(),
            &temp.path().join("work"),
            deadline(),
        )
        .expect("runtime");
    }
}

#[test]
fn unsafe_aqua_requests_fail_before_any_probe_or_candidate() {
    let marker_root = tempfile::tempdir().expect("marker root");
    let marker = marker_root.path().join("probe-ran");
    let program = format!(
        "#!/bin/sh\ntouch '{}'\nprintf 'fixture-tool 7.8.9\\n'\n",
        marker.display()
    );
    let (temp, request) = aqua_fixture(
        "velnor-fixture/fixture-tool/fixture-tool",
        "7.8.9",
        "fixture-tool",
        program.as_bytes(),
    );
    let source = json!({"kind":"cargo","default_features":true,"features":[],"installation":{"mode":"source","source_lock_sha256":"a".repeat(64)}});
    let valid_url = fixture_url("v7.8.9");
    let dependency = valid_url.clone();
    let mut latest = request.clone();
    replace(&mut latest, "/tools/0/recipe/version", json!("latest"));
    set_aqua_url(&mut latest, &fixture_url("latest"));
    let cases = [
        changed(
            &request,
            "/tools/0/recipe/backend/package",
            json!("owner/../tool"),
        ),
        changed(
            &request,
            "/tools/0/recipe/backend/package",
            json!("velnor-fixture/fixture-tool/fixture-tool/extra"),
        ),
        with_aqua_url(&request, &fixture_url("v7.8.8")),
        with_aqua_url(
            &request,
            "https://github.com/velnor-fixture/fixture-tool/releases/download/v7.8.9/tool.tar.gz?mirror=1",
        ),
        with_aqua_url(
            &request,
            &valid_url.replace("velnor-fixture/fixture-tool", "other/fixture-tool"),
        ),
        with_aqua_url(
            &request,
            &valid_url.replace("github.com", "example.invalid"),
        ),
        with_aqua_url(&request, &format!("{valid_url}#mirror=1")),
        with_aqua_url(&request, &valid_url.replace("tool.tar.gz", "%74ool.tar.gz")),
        latest,
        changed(&request, "/tools/0/recipe/options", source),
        changed(
            &request,
            "/tools/0/recipe/platforms/0/dependency_artifacts",
            json!([{"url":dependency}]),
        ),
        changed(
            &request,
            "/tools/0/recipe/platforms/0/executables/0/probe",
            json!({"kind":"shellcheck_version","expected":"not a ShellCheck receipt"}),
        ),
    ];
    for case in cases {
        assert!(run(temp.path(), &case).is_err(), "{case}");
        assert!(!marker.exists(), "must fail before execution: {case}");
        assert!(!temp.path().join("candidate.json").exists());
    }
}

#[test]
fn generic_aqua_rejects_wrong_output_mutation_and_archive_hash() {
    for program in [
        b"#!/bin/sh\nprintf 'jactionlint 2.3.1\\n'\n".as_slice(),
        b"#!/bin/sh\np=\"${0%/*}/../support\"\nchmod u+w \"$p\"\nprintf changed > \"$p\"\nprintf 'jactionlint 2.3.0\\n'\n".as_slice(),
    ] {
        let (temp, request) = jactionlint_fixture(program);
        assert_rejected(temp.path(), &request);
    }
    let marker_root = tempfile::tempdir().expect("marker root");
    let marker = marker_root.path().join("probe-ran");
    let program = format!(
        "#!/bin/sh\ntouch '{}'\nprintf 'jactionlint 2.3.0\\n'\n",
        marker.display()
    );
    let (temp, mut request) = jactionlint_fixture(program.as_bytes());
    request["tools"][0]["archives"][0]["sha256"] = json!("a".repeat(64));
    assert_rejected(temp.path(), &request);
    assert!(!marker.exists());
}

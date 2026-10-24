//! Authoring safety and runtime parity, with executable test-only archive fixtures.
use super::*;
use flate2::{Compression, write::GzEncoder};
use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;

fn native() -> CheckPlatform {
    serde_json::from_value(json!(
        match (std::env::consts::OS, std::env::consts::ARCH) {
            ("macos", "aarch64") => "macos_arm64",
            ("macos", "x86_64") => "macos_x64",
            ("linux", "aarch64") => "linux_arm64",
            _ => "linux_x64",
        }
    ))
    .expect("native")
}

fn deadline() -> CheckDeadline {
    CheckDeadline::after(Duration::from_secs(60)).expect("deadline")
}

fn archive(path: &Path, entries: &[(&str, &[u8], u32)]) {
    let encoder = GzEncoder::new(
        fs::File::create(path).expect("archive"),
        Compression::fast(),
    );
    let mut builder = tar::Builder::new(encoder);
    for (name, bytes, mode) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(*mode);
        header.set_cksum();
        builder
            .append_data(&mut header, name, *bytes)
            .expect("entry");
    }
    builder.into_inner().expect("tar").finish().expect("gzip");
}

fn fixture(program: &[u8]) -> (tempfile::TempDir, Value) {
    let temp = tempfile::tempdir().expect("temp");
    let path = temp.path().join("tool.tar.gz");
    archive(
        &path,
        &[
            ("release/cargo-zigbuild", program, 0o755),
            ("release/support", b"data", 0o644),
        ],
    );
    let hash = acquisition::sha256_with_deadline(&fs::read(&path).expect("bytes"), deadline())
        .expect("hash");
    let url = "https://github.com/rust-cross/cargo-zigbuild/releases/download/v0.23.4/tool.tar.gz";
    let request = json!({"schema_version":1,"platform":native(),"timeout_seconds":60,"tools":[{
        "recipe":{"id":"zigbuild","backend":{"kind":"aqua","package":"rust-cross/cargo-zigbuild"},
            "version":"0.23.4","options":{"kind":"default"},
            "depends_on":[],"platforms":[{"platform":native(),"artifacts":[{"url":url}],
                "dependency_artifacts":[],"executables":[{"name":"cargo-zigbuild","path":"cargo-zigbuild",
                    "probe":{"kind":"version","expected":"cargo-zigbuild 0.23.4"}}]}]},
        "archives":[{"path":path,"url":url,"sha256":hash}]}]});
    (temp, request)
}

fn run(temp: &Path, request: &Value) -> Result<PathBuf, OrchestratorError> {
    let input = temp.join("request.json");
    fs::write(&input, serde_json::to_vec(request).expect("request")).expect("write");
    let output = temp.join("candidate.json");
    qualify_tools(&input, &temp.join("work"), &output)?;
    Ok(output)
}

const PROGRAM: &[u8] = b"#!/bin/sh\nprintf 'cargo-zigbuild 0.23.4\\n'\n";

#[test]
fn candidate_uses_complete_runtime_layout_and_expected_hash_validation() {
    let (temp, request) = fixture(PROGRAM);
    let output = run(temp.path(), &request).expect("candidate");
    let value: Value = serde_json::from_slice(&fs::read(output).expect("output")).expect("json");
    assert_eq!(value["evidence_kind"], "qualified_tool_authoring_candidate");
    let tools: Vec<QualifiedTool> =
        serde_json::from_value(value["qualified_tools"].clone()).expect("tools");
    validate_qualified_tools(&tools, "candidate").expect("complete pins");
    assert_eq!(value["probes"][0]["stdout"], "cargo-zigbuild 0.23.4\n");
    let tool = &tools[0];
    let runtime = temp
        .path()
        .canonicalize()
        .expect("canonical")
        .join("runtime");
    fs::create_dir(&runtime).expect("runtime");
    let root = runtime.join("unpacked");
    let local = PathBuf::from(
        request["tools"][0]["archives"][0]["path"]
            .as_str()
            .expect("path"),
    );
    acquisition::archive::extract_archive(
        &local,
        &root,
        &tool.platforms[0].artifacts[0].url,
        &mut acquisition::archive::ArchiveBudget::new(),
        deadline(),
    )
    .expect("extract");
    let prefix = runtime.join("prefix");
    acquisition::layout::normalize_payload(tool, native(), &[root], &prefix, deadline())
        .expect("runtime normalize");
    acquisition::tree::freeze_tree(&prefix, deadline()).expect("freeze");
    assert_eq!(
        acquisition::tree::tree_sha256(&prefix, deadline()).expect("tree"),
        tool.platforms[0].install_tree_sha256
    );
    let mut wrong = tool.clone();
    wrong.platforms[0].executables[0].sha256 = "a".repeat(64);
    let home = temp.path().join("work");
    assert!(acquisition::validate_installations(&[wrong], native(), &home, deadline()).is_err());
    let mut wrong = tool.clone();
    wrong.platforms[0].install_tree_sha256 = "a".repeat(64);
    assert!(acquisition::validate_installations(&[wrong], native(), &home, deadline()).is_err());
}

#[test]
fn all_declared_archive_hashes_are_checked_before_any_probe() {
    let (temp, mut request) = fixture(PROGRAM);
    let marker = temp.path().join("probe-ran");
    let program = format!(
        "#!/bin/sh\ntouch '{}'\nprintf 'cargo-zigbuild 0.23.4\\n'\n",
        marker.display()
    );
    let path = temp.path().join("tool.tar.gz");
    archive(&path, &[("cargo-zigbuild", program.as_bytes(), 0o755)]);
    request["tools"][0]["archives"][0]["sha256"] = json!("a".repeat(64));
    assert!(
        run(temp.path(), &request)
            .expect_err("hash")
            .to_string()
            .contains("archive_sha256_mismatch")
    );
    assert!(!marker.exists());
    assert!(!temp.path().join("work/tools/zigbuild/unpacked/0").exists());
    assert!(!temp.path().join("candidate.json").exists());
}

#[test]
fn request_rejects_unknown_duplicate_bounds_hash_fields_platform_and_dag() {
    let (_temp, request) = fixture(PROGRAM);
    let mut cases = Vec::new();
    for (key, value) in [
        ("schema_version", json!(2)),
        ("timeout_seconds", json!(0)),
        ("timeout_seconds", json!(3601)),
        ("tools", json!([])),
        ("unknown", json!(1)),
    ] {
        let mut case = request.clone();
        case[key] = value;
        cases.push(case);
    }
    let other = if native() == CheckPlatform::LinuxX64 {
        "macos_arm64"
    } else {
        "linux_x64"
    };
    let mut case = request.clone();
    case["platform"] = json!(other);
    cases.push(case);
    for field in ["install_tree_sha256", "sha256"] {
        let mut case = request.clone();
        case["tools"][0]["recipe"][field] = json!("a".repeat(64));
        cases.push(case);
    }
    let mut case = request.clone();
    case["tools"][0]["recipe"]["depends_on"] = json!(["missing"]);
    cases.push(case);
    let mut case = request.clone();
    case["tools"]
        .as_array_mut()
        .expect("array")
        .push(request["tools"][0].clone());
    cases.push(case);
    let mut case = request.clone();
    case["tools"][0]["archives"]
        .as_array_mut()
        .expect("array")
        .push(request["tools"][0]["archives"][0].clone());
    cases.push(case);
    for case in cases {
        assert!(
            request::parse(&serde_json::to_vec(&case).expect("json")).is_err(),
            "{case}"
        );
    }
    let bytes = serde_json::to_string(&request).expect("json");
    let duplicate = bytes.replacen('{', "{\"schema_version\":1,", 1);
    assert!(request::parse(duplicate.as_bytes()).is_err());
}

#[test]
fn work_output_and_local_archive_paths_fail_closed() {
    let (temp, request) = fixture(PROGRAM);
    let input = temp.path().join("request.json");
    fs::write(&input, serde_json::to_vec(&request).expect("json")).expect("request");
    let work = temp.path().join("work");
    let output = temp.path().join("candidate.json");
    fs::create_dir(&work).expect("work");
    assert!(qualify_tools(&input, &work, &output).is_err());
    fs::remove_dir(&work).expect("remove");
    fs::write(&output, b"preserve").expect("output");
    assert!(qualify_tools(&input, &work, &output).is_err());
    assert_eq!(fs::read(&output).expect("preserve"), b"preserve");
    assert!(qualify_tools(&input, Path::new("relative"), &output).is_err());
    assert!(qualify_tools(&input, &work, &work.join("candidate.json")).is_err());
    #[cfg(unix)]
    {
        let archive = temp.path().join("tool.tar.gz");
        fs::rename(&archive, temp.path().join("actual.tar.gz")).expect("rename");
        std::os::unix::fs::symlink("actual.tar.gz", &archive).expect("symlink");
        fs::remove_file(&output).expect("remove output");
        assert!(qualify_tools(&input, &work, &output).is_err());
        assert!(!output.exists());
    }
}

#[test]
fn probe_output_and_support_file_mutation_reject_candidate() {
    for program in [
        b"#!/bin/sh\nprintf 'cargo-zigbuild 0.23.5\\n'\n".as_slice(),
        b"#!/bin/sh\np=\"${0%/*}/support\"\nchmod u+w \"$p\"\nprintf changed > \"$p\"\nprintf 'cargo-zigbuild 0.23.4\\n'\n".as_slice(),
    ] {
        let (temp, request) = fixture(program);
        assert!(run(temp.path(), &request).is_err());
        assert!(!temp.path().join("candidate.json").exists());
    }
}

#[path = "qualification_candidate_admission_tests.rs"]
mod admission;
#[path = "qualification_candidate_real_tests.rs"]
mod real;
#[path = "qualification_candidate_rust_tests.rs"]
mod rust_components;

#[path = "qualification_candidate_aqua_tests.rs"]
mod aqua;

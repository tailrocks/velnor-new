//! Failure witnesses through the public authoring entry point.
use super::*;
use std::os::unix::fs::PermissionsExt as _;

fn rehash(temp: &Path, request: &mut Value) {
    let bytes = fs::read(temp.join("tool.tar.gz")).expect("archive");
    request["tools"][0]["archives"][0]["sha256"] =
        json!(acquisition::sha256_with_deadline(&bytes, deadline()).expect("hash"));
}

#[test]
fn malformed_archive_ambiguous_roots_and_executable_links_reject() {
    for mode in ["malformed", "ambiguous", "link"] {
        let (temp, mut request) = fixture(PROGRAM);
        let path = temp.path().join("tool.tar.gz");
        match mode {
            "malformed" => fs::write(&path, b"not an archive").expect("bad bytes"),
            "ambiguous" => archive(
                &path,
                &[
                    ("cargo-zigbuild", PROGRAM, 0o755),
                    ("wrapper/cargo-zigbuild", PROGRAM, 0o755),
                ],
            ),
            _ => {
                let encoder = GzEncoder::new(
                    fs::File::create(&path).expect("archive"),
                    Compression::fast(),
                );
                let mut builder = tar::Builder::new(encoder);
                let mut header = tar::Header::new_gnu();
                header.set_size(PROGRAM.len() as u64);
                header.set_mode(0o755);
                header.set_cksum();
                builder
                    .append_data(&mut header, "payload", PROGRAM)
                    .expect("program");
                let mut header = tar::Header::new_gnu();
                header.set_entry_type(tar::EntryType::Symlink);
                header.set_size(0);
                header.set_mode(0o777);
                header.set_link_name("payload").expect("link");
                header.set_cksum();
                builder
                    .append_data(&mut header, "cargo-zigbuild", b"".as_slice())
                    .expect("link");
                builder.into_inner().expect("tar").finish().expect("gzip");
            }
        }
        rehash(temp.path(), &mut request);
        assert!(run(temp.path(), &request).is_err(), "{mode}");
        assert!(!temp.path().join("candidate.json").exists());
    }
}

#[test]
fn late_bad_archive_prevents_earlier_tool_probe() {
    let (temp, mut request) = fixture(PROGRAM);
    let marker = temp.path().join("probe-ran");
    let program = format!(
        "#!/bin/sh\ntouch '{}'\nprintf 'cargo-zigbuild 0.23.4\\n'\n",
        marker.display()
    );
    archive(
        &temp.path().join("tool.tar.gz"),
        &[("cargo-zigbuild", program.as_bytes(), 0o755)],
    );
    rehash(temp.path(), &mut request);
    let mut second = request["tools"][0].clone();
    second["recipe"]["id"] = json!("later");
    second["recipe"]["depends_on"] = json!(["zigbuild"]);
    second["archives"][0]["sha256"] = json!("a".repeat(64));
    request["tools"].as_array_mut().expect("tools").push(second);
    assert!(run(temp.path(), &request).is_err());
    assert!(!marker.exists());
    assert!(!temp.path().join("work/tools/zigbuild/prefix").exists());
}

#[test]
fn later_probe_cannot_mutate_previously_qualified_dependency() {
    let (temp, mut request) = fixture(PROGRAM);
    let path = temp.path().join("later.tar.gz");
    let program = b"#!/bin/sh\np=\"$HOME/tools/zigbuild/prefix/support\"\nchmod u+w \"$p\"\nprintf changed > \"$p\"\nprintf 'cargo-zigbuild 0.23.4\\n'\n";
    archive(
        &path,
        &[("cargo-zigbuild-later", program.as_slice(), 0o755)],
    );
    let mut second = request["tools"][0].clone();
    second["recipe"]["id"] = json!("later");
    second["recipe"]["depends_on"] = json!(["zigbuild"]);
    second["recipe"]["platforms"][0]["executables"][0]["name"] = json!("cargo-zigbuild-later");
    second["recipe"]["platforms"][0]["executables"][0]["path"] = json!("cargo-zigbuild-later");
    second["archives"][0]["path"] = json!(path);
    second["archives"][0]["sha256"] = json!(
        acquisition::sha256_with_deadline(&fs::read(&path).expect("archive"), deadline())
            .expect("hash")
    );
    request["tools"].as_array_mut().expect("tools").push(second);
    assert!(
        run(temp.path(), &request)
            .expect_err("mutation")
            .to_string()
            .contains("install_tree_sha256")
    );
    assert!(!temp.path().join("candidate.json").exists());
}

#[test]
fn runtime_final_pass_rejects_support_executable_and_earlier_prefix_mutations() {
    for relative in ["support", "cargo-zigbuild"] {
        let (temp, request) = fixture(PROGRAM);
        let output = run(temp.path(), &request).expect("candidate");
        let candidate: Value =
            serde_json::from_slice(&fs::read(output).expect("output")).expect("json");
        let tools: Vec<QualifiedTool> =
            serde_json::from_value(candidate["qualified_tools"].clone()).expect("tools");
        let work = temp.path().canonicalize().expect("canonical").join("work");
        acquisition::validate_installations(&tools, native(), &work, deadline())
            .expect("unchanged");
        let path = work.join("tools/zigbuild/prefix").join(relative);
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).expect("mutable by same uid");
        fs::write(path, b"mutated").expect("mutation");
        assert!(acquisition::validate_installations(&tools, native(), &work, deadline()).is_err());
    }
}

#[test]
fn earlier_probe_cannot_change_later_staged_archive() {
    let (temp, mut request) = fixture(PROGRAM);
    let program = b"#!/bin/sh\nprintf changed > \"$HOME/tools/later/verified/0\"\nprintf 'cargo-zigbuild 0.23.4\\n'\n";
    archive(
        &temp.path().join("tool.tar.gz"),
        &[("cargo-zigbuild", program.as_slice(), 0o755)],
    );
    rehash(temp.path(), &mut request);
    let mut second = request["tools"][0].clone();
    second["recipe"]["id"] = json!("later");
    second["recipe"]["depends_on"] = json!(["zigbuild"]);
    request["tools"].as_array_mut().expect("tools").push(second);
    assert!(
        run(temp.path(), &request)
            .expect_err("stage changed")
            .to_string()
            .contains("staged_archive_sha256_mismatch")
    );
    assert!(!temp.path().join("candidate.json").exists());
}

#[test]
fn final_validation_restores_readonly_projection_without_changing_identity() {
    let (temp, request) = fixture(PROGRAM);
    let output = run(temp.path(), &request).expect("candidate");
    let candidate: Value =
        serde_json::from_slice(&fs::read(output).expect("output")).expect("json");
    let tools: Vec<QualifiedTool> =
        serde_json::from_value(candidate["qualified_tools"].clone()).expect("tools");
    let work = temp.path().canonicalize().expect("canonical").join("work");
    let path = work.join("tools/zigbuild/prefix/support");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).expect("chmod");
    acquisition::validate_installations(&tools, native(), &work, deadline())
        .expect("same identity");
    assert_eq!(
        fs::metadata(path).expect("metadata").permissions().mode() & 0o777,
        0o400
    );
}

#[test]
fn fifo_request_and_archive_reject_without_blocking_open() {
    let (temp, request) = fixture(PROGRAM);
    let fifo = temp.path().join("request-fifo");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .expect("fixture mkfifo")
            .success()
    );
    assert!(
        qualify_tools(
            &fifo,
            &temp.path().join("new-work"),
            &temp.path().join("out.json")
        )
        .is_err()
    );
    let archive = temp.path().join("tool.tar.gz");
    fs::remove_file(&archive).expect("remove archive");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&archive)
            .status()
            .expect("fixture mkfifo")
            .success()
    );
    assert!(run(temp.path(), &request).is_err());
    assert!(!temp.path().join("candidate.json").exists());
}

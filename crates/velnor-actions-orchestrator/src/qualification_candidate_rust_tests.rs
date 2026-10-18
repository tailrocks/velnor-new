//! Rust component manifests remain authoritative in both authoring and runtime.
use super::*;

fn rust_request(temp: &Path, missing_std: bool) -> Value {
    let target = native().target();
    let cargo = b"#!/bin/sh\nprintf 'cargo 1.99.1\\n'\n";
    let expected = format!(
        "rustc 1.99.1\nbinary: rustc\ncommit-hash: {}\nhost: {target}\nrelease: 1.99.1",
        "a".repeat(40)
    );
    let rustc = format!("#!/bin/sh\nprintf '%s\\n' '{expected}'\n");
    let std_name = format!("rust-std-{target}");
    let std_path = format!("lib/rustlib/{target}/lib/libstd.rlib");
    let components = if missing_std {
        "cargo\nrustc\n".to_owned()
    } else {
        format!("cargo\nrustc\n{std_name}\n")
    };
    let entries = [
        ("dist/components".to_owned(), components.into_bytes(), 0o644),
        (
            "dist/cargo/manifest.in".to_owned(),
            b"file:bin/cargo\n".to_vec(),
            0o644,
        ),
        ("dist/cargo/bin/cargo".to_owned(), cargo.to_vec(), 0o755),
        (
            "dist/rustc/manifest.in".to_owned(),
            b"file:bin/rustc\n".to_vec(),
            0o644,
        ),
        ("dist/rustc/bin/rustc".to_owned(), rustc.into_bytes(), 0o755),
        (
            format!("dist/{std_name}/manifest.in"),
            format!("file:{std_path}\n").into_bytes(),
            0o644,
        ),
        (
            format!("dist/{std_name}/{std_path}"),
            b"stdlib fixture".to_vec(),
            0o644,
        ),
    ];
    let refs: Vec<_> = entries
        .iter()
        .map(|(name, bytes, mode)| (name.as_str(), bytes.as_slice(), *mode))
        .collect();
    let path = temp.join("rust.tar.gz");
    archive(&path, &refs);
    let hash = acquisition::sha256_with_deadline(&fs::read(&path).expect("archive"), deadline())
        .expect("hash");
    let url = format!("https://static.rust-lang.org/dist/rust-1.99.1-{target}.tar.gz");
    json!({"schema_version":1,"platform":native(),"timeout_seconds":60,"tools":[{
        "recipe":{"id":"rust","backend":{"kind":"core","tool":"rust"},"version":"1.99.1",
            "options":{"kind":"rust","components":[],"targets":[]},"depends_on":[],
            "platforms":[{"platform":native(),"artifacts":[{"url":url}],"dependency_artifacts":[],
                "executables":[{"name":"cargo","path":"bin/cargo","probe":{"kind":"version","expected":"cargo 1.99.1"}},
                    {"name":"rustc","path":"bin/rustc","probe":{"kind":"rustc_verbose","expected":expected}}]}]},
        "archives":[{"path":path,"url":url,"sha256":hash}]}]})
}

#[test]
fn rust_component_assembly_and_missing_target_component_drift() {
    let temp = tempfile::tempdir().expect("temp");
    let request = rust_request(temp.path(), false);
    run(temp.path(), &request).expect("assembled cargo/rustc/host std");
    let temp = tempfile::tempdir().expect("temp");
    let request = rust_request(temp.path(), true);
    assert!(
        run(temp.path(), &request)
            .expect_err("std missing")
            .to_string()
            .contains("rust_component_missing")
    );
    assert!(!temp.path().join("candidate.json").exists());
}

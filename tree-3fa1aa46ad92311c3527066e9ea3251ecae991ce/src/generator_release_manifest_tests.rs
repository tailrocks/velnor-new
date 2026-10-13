use super::{ReleaseContext, assemble_for, sha256_lower, validate_inventory, verify_for};
use std::fs;
use velnor_actions_contract::{
    RELEASE_MANIFEST_FILENAME, ReleaseManifest, SUPPORTED_TARGETS, asset_filename,
};

fn fixture(test: &str) -> (tempfile::TempDir, ReleaseContext) {
    let root = tempfile::tempdir().expect("scratch directory");
    let assets = root.path().join("assets");
    fs::create_dir(&assets).expect("asset directory");
    let context = ReleaseContext {
        asset_dir: assets,
        version: "0.1.0".to_owned(),
        tag: "v0.1.0".to_owned(),
        repository: "tailrocks/velnor-new".to_owned(),
        source: "ab".repeat(20),
    };
    write_assets(&context).expect(test);
    (root, context)
}

fn write_assets(context: &ReleaseContext) -> Result<(), std::io::Error> {
    for target in SUPPORTED_TARGETS {
        let name = asset_filename(&context.version, target);
        let bytes = format!("release bytes for {target}\n");
        fs::write(context.asset_dir.join(&name), &bytes)?;
        let sum = sha256_lower(bytes.as_bytes());
        fs::write(
            context.asset_dir.join(format!("{name}.sha256")),
            format!("{sum}  {name}\n"),
        )?;
    }
    Ok(())
}

#[test]
fn assembly_is_canonical_and_verification_binds_all_three_assets() {
    let (_root, context) = fixture("valid release assets");
    assemble_for(&context).expect("assemble canonical manifest");
    verify_for(&context).expect("verify actual assets");
    let bytes =
        fs::read(context.asset_dir.join(RELEASE_MANIFEST_FILENAME)).expect("manifest bytes");
    let text = String::from_utf8(bytes.clone()).expect("manifest utf8");
    let manifest =
        ReleaseManifest::parse_json(&text, RELEASE_MANIFEST_FILENAME).expect("manifest schema");
    manifest
        .validate(RELEASE_MANIFEST_FILENAME)
        .expect("manifest contract");
    assert_eq!(manifest.version, context.version);
    assert_eq!(manifest.repository, context.repository);
    assert_eq!(manifest.commit, context.source);
    assert_eq!(manifest.targets.len(), SUPPORTED_TARGETS.len());
    for (record, target) in manifest.targets.iter().zip(SUPPORTED_TARGETS) {
        assert_eq!(record.target, target);
        assert_eq!(
            record.artifact,
            format!(
                "https://github.com/{}/releases/download/{}/{}",
                context.repository,
                context.tag,
                asset_filename(&context.version, target)
            )
        );
    }
    assert_eq!(
        serde_json::to_vec(&manifest).expect("canonical JSON"),
        bytes
    );
    assert!(
        assemble_for(&context).is_err(),
        "assembly cannot replace bytes"
    );
}

#[test]
fn assembly_rejects_missing_extra_and_mismatched_sidecar_assets() {
    let (_root, missing) = fixture("missing asset");
    let missing_asset = missing.asset_dir.join(format!(
        "{}.sha256",
        asset_filename(&missing.version, SUPPORTED_TARGETS[2])
    ));
    fs::remove_file(missing_asset).expect("remove sidecar");
    assert!(assemble_for(&missing).is_err());

    let (_root, extra) = fixture("extra asset");
    fs::write(extra.asset_dir.join("unexpected"), b"extra").expect("write extra");
    assert!(assemble_for(&extra).is_err());

    let (_root, bad_sidecar) = fixture("bad sidecar");
    let sidecar = bad_sidecar.asset_dir.join(format!(
        "{}.sha256",
        asset_filename(&bad_sidecar.version, SUPPORTED_TARGETS[1])
    ));
    fs::write(sidecar, b"00  wrong\n").expect("corrupt sidecar");
    assert!(
        assemble_for(&bad_sidecar)
            .expect_err("wrong digest must fail")
            .to_string()
            .contains("release_sidecar_mismatch")
    );
}

#[test]
fn verification_rejects_changed_assets_and_context_mismatch() {
    let (_root, changed) = fixture("changed asset");
    assemble_for(&changed).expect("assemble");
    let binary = changed
        .asset_dir
        .join(asset_filename(&changed.version, SUPPORTED_TARGETS[2]));
    fs::write(binary, b"changed binary").expect("change x64 bytes");
    assert!(
        verify_for(&changed)
            .expect_err("changed bytes must fail")
            .to_string()
            .contains("release_sidecar_mismatch")
    );

    let (_root, wrong_tag) = fixture("wrong tag");
    let mut wrong_tag = wrong_tag;
    wrong_tag.tag = "generator-0123456789abcdef0123456789abcdef01234567".to_owned();
    assert!(
        verify_for(&wrong_tag)
            .expect_err("non-version tag must fail")
            .to_string()
            .contains("release_tag_version_mismatch")
    );

    let (_root, wrong_source) = fixture("wrong source");
    assemble_for(&wrong_source).expect("assemble");
    let mut wrong_source = wrong_source;
    wrong_source.source = "cd".repeat(20);
    assert!(
        verify_for(&wrong_source)
            .expect_err("source mismatch must fail")
            .to_string()
            .contains("manifest_asset_binding_mismatch")
    );
}

#[test]
fn inventory_rejects_directories() {
    let (_root, directory) = fixture("directory asset");
    let name = asset_filename(&directory.version, SUPPORTED_TARGETS[0]);
    fs::remove_file(directory.asset_dir.join(&name)).expect("remove binary");
    fs::create_dir(directory.asset_dir.join(&name)).expect("create directory as binary");
    assert!(validate_inventory(&directory, false).is_err());
}

#[cfg(unix)]
#[test]
fn inventory_rejects_symlinks() {
    let (root, linked) = fixture("symlink asset");
    let name = asset_filename(&linked.version, SUPPORTED_TARGETS[0]);
    let path = linked.asset_dir.join(&name);
    let target = root.path().join("real-target");
    let original = fs::read(&path).expect("read original asset");
    fs::rename(&path, &target).expect("move target");
    std::os::unix::fs::symlink(&target, &path).expect("plant asset symlink");
    let error = assemble_for(&linked).expect_err("asset symlink must fail");
    assert!(error.to_string().contains("release_asset_not_regular_file"));
    assert_eq!(fs::read(&target).expect("read preserved target"), original);
    assert!(!linked.asset_dir.join(RELEASE_MANIFEST_FILENAME).exists());
}

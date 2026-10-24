use super::assets::public_asset_size_limits;
use super::*;
use velnor_actions_mise::CheckDeadline;

#[test]
fn public_asset_limits_are_kind_specific_and_closed_over_plan_names() {
    let limits = public_asset_size_limits(&jackin_historical_plan()).expect("plan asset bounds");
    assert_eq!(limits.get("release.json"), Some(&(1024 * 1024)));
    assert_eq!(limits.get("release.json.bundle"), Some(&(8 * 1024 * 1024)));
    assert_eq!(
        limits.get("jackin-aarch64-apple-darwin.tar.gz"),
        Some(&(1024 * 1024 * 1024))
    );
    assert_eq!(
        limits.get("jackin-aarch64-apple-darwin.tar.gz.sbom.json"),
        Some(&(32 * 1024 * 1024))
    );
    assert_eq!(
        limits.get("jackin-aarch64-apple-darwin.tar.gz.sha256"),
        Some(&(1024 * 1024))
    );
    assert_eq!(
        limits.get("jackin-aarch64-apple-darwin.tar.gz.bundle"),
        Some(&(8 * 1024 * 1024))
    );
    assert_eq!(
        limits.get("capsule-manifest.json.bundle"),
        Some(&(8 * 1024 * 1024))
    );
    let expected = [
        "capsule-manifest.json",
        "capsule-manifest.json.bundle",
        "jackin-aarch64-apple-darwin.tar.gz",
        "jackin-aarch64-apple-darwin.tar.gz.bundle",
        "jackin-aarch64-apple-darwin.tar.gz.sbom.json",
        "jackin-aarch64-apple-darwin.tar.gz.sha256",
        "jackin-aarch64-unknown-linux-gnu.tar.gz",
        "jackin-aarch64-unknown-linux-gnu.tar.gz.bundle",
        "jackin-aarch64-unknown-linux-gnu.tar.gz.sbom.json",
        "jackin-aarch64-unknown-linux-gnu.tar.gz.sha256",
        "jackin-capsule-aarch64-unknown-linux-gnu.tar.gz",
        "jackin-capsule-aarch64-unknown-linux-gnu.tar.gz.bundle",
        "jackin-capsule-aarch64-unknown-linux-gnu.tar.gz.sbom.json",
        "jackin-capsule-aarch64-unknown-linux-gnu.tar.gz.sha256",
        "jackin-capsule-x86_64-unknown-linux-gnu.tar.gz",
        "jackin-capsule-x86_64-unknown-linux-gnu.tar.gz.bundle",
        "jackin-capsule-x86_64-unknown-linux-gnu.tar.gz.sbom.json",
        "jackin-capsule-x86_64-unknown-linux-gnu.tar.gz.sha256",
        "jackin-x86_64-apple-darwin.tar.gz",
        "jackin-x86_64-apple-darwin.tar.gz.bundle",
        "jackin-x86_64-apple-darwin.tar.gz.sbom.json",
        "jackin-x86_64-apple-darwin.tar.gz.sha256",
        "jackin-x86_64-unknown-linux-gnu.tar.gz",
        "jackin-x86_64-unknown-linux-gnu.tar.gz.bundle",
        "jackin-x86_64-unknown-linux-gnu.tar.gz.sbom.json",
        "jackin-x86_64-unknown-linux-gnu.tar.gz.sha256",
        "release.json",
        "release.json.bundle",
        "SHA256SUMS",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        limits
            .keys()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>(),
        expected
    );
    assert_eq!(limits.len(), 29);
    assert_eq!(limits.get("not-in-the-plan.bin"), None);
}

#[test]
fn held_public_asset_reads_the_admitted_inode_and_rejects_path_replacement() {
    let directory = tempfile::tempdir().expect("temporary custody directory");
    let root = directory
        .path()
        .canonicalize()
        .expect("canonical custody root");
    let asset_path = root.join("jackin-aarch64-apple-darwin.tar.gz");
    std::fs::write(&asset_path, b"verified archive bytes").expect("write verified bytes");
    let deadline =
        CheckDeadline::after(std::time::Duration::from_secs(10)).expect("custody deadline");
    let held = crate::filesystem_owner::held_directory::HeldDirectory::capture(&root, deadline)
        .expect("hold asset inventory");
    assert_eq!(
        held.read_file("jackin-aarch64-apple-darwin.tar.gz", 1024)
            .expect("read held inode"),
        b"verified archive bytes"
    );

    std::fs::rename(&asset_path, root.join("original")).expect("move held path");
    std::fs::write(&asset_path, b"substituted bytes").expect("create substitute");
    assert!(
        held.read_file("jackin-aarch64-apple-darwin.tar.gz", 1024)
            .is_err()
    );
}

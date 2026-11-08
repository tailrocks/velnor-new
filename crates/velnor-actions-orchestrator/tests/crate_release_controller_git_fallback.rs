// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::release::crate_release_source::CrateReleaseSourcePackage;

#[test]
fn versioned_git_dependency_packages_with_registry_fallback()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let dependency = temp.path().join("dependency");
    let source = temp.path().join("source");
    let target = temp.path().join("target");
    let cargo_home = temp.path().join("cargo-home");
    fs::create_dir(&dependency)?;
    fs::create_dir(&source)?;
    fs::create_dir(&target)?;
    fs::create_dir(&cargo_home)?;
    fs::create_dir(dependency.join("src"))?;
    fs::write(
        dependency.join("Cargo.toml"),
        "[package]\nname='fixture_git_fallback'\nversion='0.1.1'\nedition='2021'\nlicense='MIT'\ndescription='fixture'\n",
    )?;
    fs::write(
        dependency.join("src/lib.rs"),
        "pub fn answer() -> u8 { 42 }\n",
    )?;
    git(&dependency, &["init", "-q"])?;
    git(&dependency, &["add", "."])?;
    git(
        &dependency,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            "fixture",
        ],
    )?;
    let output = Command::new(env!("CARGO"))
        .args(["package", "--allow-dirty", "--no-verify", "--target-dir"])
        .arg(&target)
        .current_dir(&dependency)
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let archive = fs::read(target.join("package/fixture_git_fallback-0.1.1.crate"))?;
    let server = SparseFixture::start("fixture_git_fallback", archive)?;
    server
        .index_visible
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let (intent, _, _, _) = make_fixture(&source)?;
    let manifest = format!(
        "[package]\nname='demo'\nversion='0.1.1'\nedition='2021'\nlicense='MIT'\ndescription='fixture'\n[dependencies]\nfixture_git_fallback={{git='file://{}',version='0.1.1'}}\n",
        dependency.display()
    );
    fs::write(source.join("Cargo.toml"), manifest)?;
    fs::write(
        cargo_home.join("config.toml"),
        format!(
            "[source.crates-io]\nreplace-with='fixture'\n[source.fixture]\nregistry='sparse+{}/'\n",
            server.url
        ),
    )?;
    let output = Command::new(env!("CARGO"))
        .args(["generate-lockfile"])
        .env("CARGO_HOME", &cargo_home)
        .current_dir(&source)
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    git(&source, &["add", "."])?;
    git(&source, &["commit", "-qm", "git dependency"])?;
    let sha = git(&source, &["rev-parse", "HEAD"])?;
    let package = CrateReleaseSourcePackage {
        name: "demo".into(),
        version: "0.1.1".into(),
        manifest: "Cargo.toml".into(),
    };
    FIXTURE_SPARSE_INDEX.with(|value| *value.borrow_mut() = Some(server.url.clone()));
    let result = crate::crate_release_controller::package_unprivileged(
        &source,
        &intent,
        &package,
        &target,
        &sha,
        true,
        test_git(),
    );
    FIXTURE_SPARSE_INDEX.with(|value| *value.borrow_mut() = None);
    let bytes = result?;
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(std::io::Cursor::new(bytes)));
    let mut normalized = String::new();
    for entry in tar.entries()? {
        let mut entry = entry?;
        if entry.path()?.as_ref() == Path::new("demo-0.1.1/Cargo.toml") {
            std::io::Read::read_to_string(&mut entry, &mut normalized)?;
        }
    }
    let value: toml::Value = toml::from_str(&normalized)?;
    let dependency = &value["dependencies"]["fixture_git_fallback"];
    assert_eq!(dependency["version"].as_str(), Some("0.1.1"));
    assert!(dependency.get("git").is_none());
    assert!(
        server
            .download_hits
            .load(std::sync::atomic::Ordering::SeqCst)
            > 0
    );
    Ok(())
}

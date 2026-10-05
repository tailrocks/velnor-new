#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use velnor_actions_workflow_renderer::steps::TOOLS_CACHE_PATHS;

use super::TempRoot;

#[test]
fn tools_cache_archive_relocates_between_workspace_depths() -> Result<(), Box<dyn std::error::Error>>
{
    let expected = [
        ".velnor/cache/mise",
        ".velnor/cache/rustup",
        ".velnor/cache/cargo/bin",
    ];
    assert_eq!(TOOLS_CACHE_PATHS, expected);
    let temp = TempRoot::new("cache-relocation")?;
    let producer = temp.0.join("producer/nested/workspace");
    let consumer = temp.0.join("consumer/workflow/repository/checkout");
    let home = temp.0.join("unrelated-home");
    fs::create_dir_all(&producer)?;
    fs::create_dir_all(&consumer)?;
    fs::create_dir_all(&home)?;
    assert_ne!(producer.components().count(), consumer.components().count());
    let archive = temp.0.join("tools.tar");
    let cargo_bin = populate_relocatable_tools(&producer)?;
    create_tools_archive(&producer, &archive)?;
    assert_tools_archive_members(&archive, &producer, &home)?;
    fs::remove_dir_all(&producer)?;
    assert!(!producer.exists(), "producer root is gone before restore");
    extract_tools_archive(&archive, &consumer)?;
    assert_relocated_tools(&consumer, &cargo_bin)?;
    assert_private_payload_locations(&consumer, &home)?;
    Ok(())
}

const RELOCATABLE_TOOLS: [(&str, &str, &str); 3] = [
    (
        ".velnor/cache/mise",
        "downloads/rust/1.98.1.tar",
        "mise install",
    ),
    (
        ".velnor/cache/rustup",
        "toolchains/1.98.1-x86_64-unknown-linux-gnu/bin/rustc",
        "rustup toolchain",
    ),
    (".velnor/cache/cargo/bin", "rustc", "cargo proxy"),
];

fn populate_relocatable_tools(producer: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    for (cache_path, suffix, contents) in RELOCATABLE_TOOLS {
        let source = producer.join(cache_path).join(suffix);
        fs::create_dir_all(source.parent().ok_or("payload parent")?)?;
        fs::write(source, contents)?;
    }
    let cargo_bin = producer.join(TOOLS_CACHE_PATHS[2]);
    let mise_install = producer
        .join(TOOLS_CACHE_PATHS[0])
        .join("installs/rust/1.98.1");
    fs::create_dir_all(mise_install.parent().ok_or("Mise install parent")?)?;
    std::os::unix::fs::symlink(&cargo_bin, &mise_install)?;
    assert_eq!(fs::read_link(&mise_install)?, cargo_bin);
    Ok(cargo_bin)
}

fn create_tools_archive(producer: &Path, archive: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let mut create = Command::new("tar");
    create.arg("-cf").arg(archive).arg("-C").arg(producer);
    create.args(TOOLS_CACHE_PATHS);
    assert!(create.status()?.success(), "archive tool payloads");
    Ok(())
}

fn assert_tools_archive_members(
    archive: &Path,
    producer: &Path,
    home: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let listing = Command::new("tar").arg("-tf").arg(archive).output()?;
    assert!(listing.status.success(), "list archive members");
    let listing = String::from_utf8(listing.stdout)?;
    for path in TOOLS_CACHE_PATHS {
        assert!(
            listing
                .lines()
                .any(|entry| entry.trim_end_matches('/') == path),
            "missing {path}"
        );
    }
    let link = ".velnor/cache/mise/installs/rust/1.98.1";
    assert!(
        listing
            .lines()
            .any(|entry| entry.trim_end_matches('/') == link)
    );
    assert!(!listing.contains(producer.to_string_lossy().as_ref()));
    assert!(!listing.contains(home.to_string_lossy().as_ref()));
    Ok(())
}

fn extract_tools_archive(
    archive: &Path,
    consumer: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let extracted = Command::new("tar")
        .arg("-xf")
        .arg(archive)
        .arg("-C")
        .arg(consumer)
        .status()?;
    assert!(extracted.success(), "extract under consumer workspace");
    Ok(())
}

fn assert_relocated_tools(
    consumer: &Path,
    cargo_bin: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let restored_mise_install = consumer
        .join(TOOLS_CACHE_PATHS[0])
        .join("installs/rust/1.98.1");
    assert!(
        fs::symlink_metadata(&restored_mise_install)?
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read_link(&restored_mise_install)?, cargo_bin);
    assert!(
        !restored_mise_install.exists(),
        "producer target stays absent"
    );
    Ok(())
}

fn assert_private_payload_locations(
    consumer: &Path,
    home: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    for (cache_path, suffix, contents) in RELOCATABLE_TOOLS {
        let payload = consumer.join(cache_path).join(suffix);
        assert!(payload.starts_with(consumer));
        assert_eq!(fs::read_to_string(payload)?, contents);
    }
    for path in [
        home.join(".local/share/mise/installs/rust/1.98.1/bin/rustc"),
        home.join(".rustup/toolchains/1.98.1-x86_64-unknown-linux-gnu/bin/rustc"),
        home.join(".cargo/bin/rustc"),
        consumer.join(".local/share/mise/installs/rust/1.98.1/bin/rustc"),
        consumer.join(".rustup/toolchains/1.98.1-x86_64-unknown-linux-gnu/bin/rustc"),
        consumer.join(".cargo/bin/rustc"),
    ] {
        assert!(
            !path.exists(),
            "legacy path stayed absent: {}",
            path.display()
        );
    }
    Ok(())
}

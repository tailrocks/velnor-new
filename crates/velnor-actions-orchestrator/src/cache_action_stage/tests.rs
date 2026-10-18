use super::{StageContext, StageReceiptClaim, create_stage, open_stage, rewrite_stage};
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

fn context(root: &Path) -> Result<StageContext, Box<dyn std::error::Error>> {
    let runner_temp = root.join("runner-temp");
    let workspace = root.join("workspace");
    std::fs::create_dir(&runner_temp)?;
    std::fs::create_dir(&workspace)?;
    let key = format!("mise-tools-v2-payload-v3-{}", "a".repeat(64));
    Ok(StageContext::normal(&runner_temp, &workspace, &key)?)
}

#[test]
fn compact_stage_claim_round_trips_and_rejects_noncanonical_numbers() {
    let claim = StageReceiptClaim::new(0x1a, 0x2b, 12, [0xab; 32]);
    let encoded = claim.encode();
    assert_eq!(encoded, format!("v1.1a.2b.12.{}", "ab".repeat(32)));
    assert_eq!(StageReceiptClaim::parse(&encoded), Ok(claim));
    assert!(StageReceiptClaim::parse(&encoded.replace(".1a.", ".01a.")).is_err());
    assert!(StageReceiptClaim::parse(&encoded.replace(".12.", ".012.")).is_err());
    assert!(StageReceiptClaim::parse(&encoded.to_uppercase()).is_err());
}

#[test]
fn stage_creation_keeps_one_descriptor_bound_file_and_rejects_replacement()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let context = context(root.path())?;
    assert_eq!(
        context.action_path().file_name(),
        Some(std::ffi::OsStr::new("payload"))
    );
    let operation_directory = context.action_path().parent().expect("operation directory");
    let digest = operation_directory
        .file_name()
        .and_then(std::ffi::OsStr::to_str);
    assert!(digest.is_some_and(|value| {
        value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    }));
    assert_eq!(
        operation_directory.parent().and_then(Path::file_name),
        Some(std::ffi::OsStr::new("cache-staging"))
    );
    assert_eq!(
        operation_directory
            .parent()
            .and_then(Path::parent)
            .and_then(Path::file_name),
        Some(std::ffi::OsStr::new("velnor"))
    );
    let mut writer = create_stage(&context)?;
    writer.write_all(b"verified transport")?;
    let claim = writer.finish()?;
    let mut reader = open_stage(&context, &claim)?;
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes)?;
    assert_eq!(bytes, b"verified transport");
    reader.revalidate()?;

    let stage_path = context.action_path();
    std::fs::remove_file(stage_path)?;
    std::fs::write(stage_path, b"foreign replacement")?;
    std::fs::set_permissions(stage_path, std::fs::Permissions::from_mode(0o600))?;
    assert!(open_stage(&context, &claim).is_err());
    assert_eq!(std::fs::read(stage_path)?, b"foreign replacement");
    Ok(())
}

#[test]
fn stage_creation_does_not_adopt_or_remove_an_existing_file()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let context = context(root.path())?;
    std::fs::create_dir_all(
        context
            .action_path()
            .parent()
            .ok_or("missing stage parent")?,
    )?;
    std::fs::write(context.action_path(), b"foreign file")?;
    std::fs::set_permissions(
        context.action_path(),
        std::fs::Permissions::from_mode(0o600),
    )?;
    assert!(create_stage(&context).is_err());
    assert_eq!(std::fs::read(context.action_path())?, b"foreign file");
    Ok(())
}

#[test]
fn dropping_a_partial_writer_removes_only_its_owned_stage_file()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let context = context(root.path())?;
    let mut writer = create_stage(&context)?;
    writer.write_all(b"partial payload")?;
    drop(writer);

    assert!(!context.action_path().exists());
    Ok(())
}

#[test]
fn dropping_a_writer_preserves_a_replacement_stage_file() -> Result<(), Box<dyn std::error::Error>>
{
    let root = tempfile::tempdir()?;
    let context = context(root.path())?;
    let writer = create_stage(&context)?;
    std::fs::remove_file(context.action_path())?;
    std::fs::write(context.action_path(), b"foreign replacement")?;
    std::fs::set_permissions(
        context.action_path(),
        std::fs::Permissions::from_mode(0o600),
    )?;
    drop(writer);

    assert_eq!(
        std::fs::read(context.action_path())?,
        b"foreign replacement"
    );
    Ok(())
}

#[test]
fn explicit_rewrite_requires_the_current_claim() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let context = context(root.path())?;
    let mut initial = create_stage(&context)?;
    initial.write_all(b"before")?;
    let old_claim = initial.finish()?;
    let mut rewrite = rewrite_stage(&context, &old_claim)?;
    rewrite.write_all(b"after")?;
    let new_claim = rewrite.finish()?;
    assert_ne!(new_claim.digest(), old_claim.digest());
    assert!(open_stage(&context, &old_claim).is_err());
    let mut reader = open_stage(&context, &new_claim)?;
    let mut actual = Vec::new();
    reader.read_to_end(&mut actual)?;
    assert_eq!(actual, b"after");
    Ok(())
}

#[test]
fn stage_creation_accepts_a_sticky_shared_temporary_ancestor()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let shared = root.path().join("shared-temp");
    std::fs::create_dir(&shared)?;
    std::fs::set_permissions(&shared, std::fs::Permissions::from_mode(0o1777))?;
    let runner_temp = shared.join("runner-temp");
    let workspace = root.path().join("workspace");
    std::fs::create_dir(&runner_temp)?;
    std::fs::create_dir(&workspace)?;
    std::fs::set_permissions(&runner_temp, std::fs::Permissions::from_mode(0o700))?;
    let key = format!("mise-tools-v2-payload-v3-{}", "b".repeat(64));
    let context = StageContext::normal(&runner_temp, &workspace, &key)?;

    let writer = create_stage(&context)?;
    let claim = writer.finish()?;
    assert_eq!(claim.length(), 0);
    assert_eq!(
        std::fs::metadata(context.action_path())?
            .permissions()
            .mode()
            & 0o7777,
        0o600
    );
    Ok(())
}

#[test]
fn stage_creation_rejects_a_writable_nonsticky_ancestor_before_creating_file()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let shared = root.path().join("shared-temp");
    std::fs::create_dir(&shared)?;
    std::fs::set_permissions(&shared, std::fs::Permissions::from_mode(0o777))?;
    let runner_temp = shared.join("runner-temp");
    let workspace = root.path().join("workspace");
    std::fs::create_dir(&runner_temp)?;
    std::fs::create_dir(&workspace)?;
    std::fs::set_permissions(&runner_temp, std::fs::Permissions::from_mode(0o700))?;
    let key = format!("mise-tools-v2-payload-v3-{}", "c".repeat(64));
    let context = StageContext::normal(&runner_temp, &workspace, &key)?;

    assert!(create_stage(&context).is_err());
    assert!(!context.action_path().exists());
    Ok(())
}

#[test]
fn stage_creation_rejects_a_symlink_replacing_the_trusted_root()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let context = context(root.path())?;
    let original = root.path().join("runner-temp");
    let moved = root.path().join("runner-temp-moved");
    std::fs::rename(&original, &moved)?;
    #[cfg(unix)]
    std::os::unix::fs::symlink(&moved, &original)?;

    assert!(create_stage(&context).is_err());
    assert!(
        !moved
            .join(
                context
                    .action_path()
                    .file_name()
                    .ok_or("missing stage leaf")?
            )
            .exists()
    );
    Ok(())
}

#[test]
fn stage_creation_rejects_a_symlink_at_the_fixed_quota_root()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let context = context(root.path())?;
    let runner_temp = root.path().join("runner-temp");
    let foreign = root.path().join("foreign");
    std::fs::create_dir(&foreign)?;
    std::os::unix::fs::symlink(&foreign, runner_temp.join("velnor"))?;

    assert!(create_stage(&context).is_err());
    assert!(!foreign.join("cache-staging").exists());
    assert!(!context.action_path().exists());
    Ok(())
}

#[test]
fn retained_stage_parent_rejects_replacement_before_a_write()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let context = context(root.path())?;
    let mut writer = create_stage(&context)?;
    let runner_temp = root.path().join("runner-temp");
    let stage_root = runner_temp.join("velnor/cache-staging");
    let moved = runner_temp.join("velnor/cache-staging-moved");
    std::fs::rename(&stage_root, &moved)?;
    std::fs::create_dir(&stage_root)?;
    std::fs::set_permissions(&stage_root, std::fs::Permissions::from_mode(0o700))?;

    assert!(
        writer
            .write_all(b"must not be written through a replaced path")
            .is_err()
    );
    assert!(!context.action_path().exists());
    let digest = context
        .action_path()
        .parent()
        .and_then(Path::file_name)
        .ok_or("missing stage operation directory")?;
    assert!(moved.join(digest).join("payload").exists());
    Ok(())
}

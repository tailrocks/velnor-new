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
        context.action_path().parent().and_then(Path::file_name),
        Some(std::ffi::OsStr::new("runner-temp"))
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

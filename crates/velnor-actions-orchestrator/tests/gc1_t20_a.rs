// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn gc1_t20_preserves_contract() -> Result<(), Box<dyn Error>> {
    let fixture = build_full_fixture()?;
    let plan = build_snapshot_plan(&fixture.inputs)?;
    check_plan_contents(&plan)?;
    let out_path = fixture.temp.path().join("payload.tar.gz");
    let packed = pack_snapshot_archive(&velnor_actions_orchestrator::PackRequest {
        plan: &plan,
        checkout_root: &fixture.checkout,
        staged_projection_root: &fixture.staged,
        out_path: &out_path,
    })?;
    if packed.archive_bytes == 0 || packed.archive_sha256.len() != 64 {
        return Err(fail("packed archive digest missing"));
    }
    let expected_members = plan.state_index.len() + plan.projection.len() + 4;
    if packed.member_count != expected_members {
        return Err(fail("packed archive member count drifted"));
    }
    if plan.manifest.entry_count != u64::try_from(plan.state_index.len() + plan.projection.len())? {
        return Err(fail("manifest entry count drifted"));
    }
    let destination = fixture.temp.path().join("restored");
    let extracted = extract_snapshot_archive(&out_path, &destination)?;
    check_extracted_indexes(&plan, &extracted)?;
    if destination.join("manifest.json").exists() || destination.join("state-index.json").exists() {
        return Err(fail("index members leaked into restored payload"));
    }
    let restored_app = destination.join("state/build/debug/app");
    if fs::read(&restored_app)? != fixture.app_bytes {
        return Err(fail("restored state bytes drifted"));
    }
    if read_mtime(&restored_app)? != fixture.app_mtime {
        return Err(fail("restored state mtime drifted"));
    }
    if mode_of(&restored_app)? != mode_of(&fixture.target.join("debug/app"))? {
        return Err(fail("restored state mode drifted"));
    }
    if read_mtime(&destination.join("state/build/debug"))? != fixture.debug_mtime {
        return Err(fail("restored state directory mtime drifted"));
    }
    if fs::read(destination.join("state/build/.mbx-rustdoc-parts/doc.json"))?
        != fixture.rustdoc_bytes
    {
        return Err(fail("restored rustdoc part drifted"));
    }
    if destination.join("state/build/disposable-store").exists() {
        return Err(fail(
            "excluded disposable root leaked into restored payload",
        ));
    }
    if !fixture.excluded.is_dir() {
        return Err(fail("fixture disposable root missing"));
    }
    if fs::read(destination.join("state/test/x86_64/out.bin"))? != fixture.out_bytes {
        return Err(fail("separate build lane bytes drifted"));
    }
    if fs::read_link(destination.join("state/build/debug/latest"))? != *Path::new("app") {
        return Err(fail("restored link identity drifted"));
    }
    if fs::read(destination.join("projection/src/lib.rs"))? != fixture.lib_bytes {
        return Err(fail("restored projection bytes drifted"));
    }
    if fs::read(destination.join("projection/src/changed.rs"))? != fixture.changed_bytes {
        return Err(fail("current projection bytes drifted"));
    }
    if fs::read(destination.join("projection/src/missing.rs"))? != fixture.missing_bytes {
        return Err(fail("missing projection fallback drifted"));
    }
    if read_mtime(&destination.join("projection/src/lib.rs"))? != fixture.lib_mtime {
        return Err(fail("restored projection mtime drifted"));
    }
    Ok(())
}

pub(crate) fn tampered_manifest(
    plan: &SnapshotPlan,
    mutate: &str,
) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut value = serde_json::to_value(&plan.manifest)?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| fail("manifest not object"))?;
    if mutate == "schema" {
        object.insert("schema".to_owned(), serde_json::Value::from(2));
    } else {
        object.insert("unexpected".to_owned(), serde_json::Value::from(1));
    }
    Ok(serde_json::to_vec(&value)?)
}

#[derive(Clone)]
pub(crate) enum CraftMember {
    File { name: String, bytes: Vec<u8> },
    Dir { name: String },
    Link { name: String, target: String },
    Hard { name: String, target: String },
    Fifo { name: String },
}

pub(crate) fn craft_archive(path: &Path, members: &[CraftMember]) -> Result<(), Box<dyn Error>> {
    let file = fs::File::create_new(path)?;
    let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
    let mut builder = tar::Builder::new(encoder);
    for member in members {
        match member {
            CraftMember::File { name, bytes } => {
                let mut header = tar::Header::new_gnu();
                header.set_entry_type(tar::EntryType::Regular);
                header.set_size(u64::try_from(bytes.len())?);
                header.set_mode(0o644);
                header.set_uid(0);
                header.set_gid(0);
                header.set_mtime(0);
                builder.append_data(&mut header, name, bytes.as_slice())?;
            }
            CraftMember::Dir { name } => {
                let mut header = tar::Header::new_gnu();
                header.set_entry_type(tar::EntryType::Directory);
                header.set_size(0);
                header.set_mode(0o755);
                header.set_uid(0);
                header.set_gid(0);
                header.set_mtime(0);
                builder.append_data(&mut header, name, io::empty())?;
            }
            CraftMember::Link { name, target } => {
                let mut header = tar::Header::new_gnu();
                header.set_entry_type(tar::EntryType::Symlink);
                header.set_size(0);
                header.set_mode(0o777);
                header.set_uid(0);
                header.set_gid(0);
                header.set_mtime(0);
                builder.append_link(&mut header, name, Path::new(target))?;
            }
            CraftMember::Hard { name, target } => {
                let mut header = tar::Header::new_gnu();
                header.set_entry_type(tar::EntryType::Link);
                header.set_size(0);
                header.set_mode(0o644);
                header.set_uid(0);
                header.set_gid(0);
                header.set_mtime(0);
                builder.append_link(&mut header, name, Path::new(target))?;
            }
            CraftMember::Fifo { name } => {
                let mut header = tar::Header::new_gnu();
                header.set_entry_type(tar::EntryType::Fifo);
                header.set_size(0);
                header.set_mode(0o644);
                header.set_uid(0);
                header.set_gid(0);
                header.set_mtime(0);
                builder.append_data(&mut header, name, io::empty())?;
            }
        }
    }
    let encoder = builder.into_inner()?;
    encoder.finish()?;
    Ok(())
}

pub(crate) fn lane_root_for<'a>(plan: &'a SnapshotPlan, member: &str) -> Option<&'a Path> {
    let rest = member.strip_prefix("state/")?;
    let (lane_id, _) = rest.split_once('/')?;
    plan.lanes
        .iter()
        .find(|lane| lane.lane_id == lane_id)
        .map(|lane| lane.root.as_path())
}

fn octal_field(field: &mut [u8], value: usize) {
    let text = format!("{:0width$o}", value, width = field.len() - 1);
    field[..text.len()].copy_from_slice(text.as_bytes());
}

pub(crate) fn raw_tar_member(name: &str, kind: u8, link: &str, data: &[u8]) -> Vec<u8> {
    let mut header = vec![0_u8; 512];
    let name_bytes = name.as_bytes();
    let name_len = name_bytes.len().min(100);
    header[..name_len].copy_from_slice(&name_bytes[..name_len]);
    octal_field(&mut header[100..108], 0o644);
    octal_field(&mut header[108..116], 0);
    octal_field(&mut header[116..124], 0);
    octal_field(&mut header[124..136], data.len());
    octal_field(&mut header[136..148], 0);
    header[156] = kind;
    let link_bytes = link.as_bytes();
    let link_len = link_bytes.len().min(100);
    header[157..157 + link_len].copy_from_slice(&link_bytes[..link_len]);
    header[257..262].copy_from_slice(b"ustar");
    header[263..265].copy_from_slice(b"00");
    header[148..156].fill(b' ');
    let sum = header
        .iter()
        .fold(0_u32, |total, byte| total + u32::from(*byte));
    let checksum = format!("{sum:06o}\0 ");
    header[148..156].copy_from_slice(checksum.as_bytes());
    let mut out = header;
    out.extend_from_slice(data);
    while !out.len().is_multiple_of(512) {
        out.push(0);
    }
    out
}

pub(crate) fn raw_archive(path: &Path, members: &[Vec<u8>]) -> Result<(), Box<dyn Error>> {
    let mut tar_data = members.concat();
    tar_data.extend_from_slice(&[0_u8; 1024]);
    let file = fs::File::create_new(path)?;
    let mut encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
    io::Write::write_all(&mut encoder, &tar_data)?;
    encoder.finish()?;
    Ok(())
}

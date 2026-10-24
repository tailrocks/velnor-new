use super::super::{BLOCK, scan_archive, write_single_member};
use super::common::{finish, member, pax_path, pax_record};
use crate::cache_action_stage::{StageContext, create_stage, open_stage};
use std::io::{Seek, SeekFrom, Write};

#[test]
fn accepts_one_regular_member_with_a_valid_posix_path_record() {
    let expected = "../../runner-temp/stage.payload";
    let mut archive = member("PaxHeaders/stage", b'x', &pax_path(expected));
    archive.extend(member("stage", b'0', b"verified payload"));
    let mut file = finish(archive);
    let mut payload = scan_archive(&mut file, expected).expect("valid one-file archive");
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut payload, &mut bytes).expect("read payload");
    assert_eq!(bytes, b"verified payload");
}

#[test]
fn creates_a_posix_path_record_for_a_long_context_member() {
    let root = tempfile::tempdir().expect("temporary root");
    let workspace = root.path().join("workspace");
    let runner_temp = root.path().join("runner-temp");
    std::fs::create_dir(&workspace).expect("create workspace");
    std::fs::create_dir(&runner_temp).expect("create runner temp");
    let mut nested = runner_temp.clone();
    for index in 0..8 {
        nested.push(format!("nested-{index}-{}", "x".repeat(30)));
        std::fs::create_dir(&nested).expect("create nested stage parent");
    }
    let key = format!("mise-tools-v2-payload-v3-{}", "a".repeat(64));
    let context = StageContext::normal(&nested, &workspace, &key).expect("build stage context");
    assert!(context.member_path().len() > 255);

    let mut writer = create_stage(&context).expect("create stage file");
    writer
        .write_all(b"long path payload")
        .expect("write stage file");
    let claim = writer.finish().expect("finish stage file");
    let mut reader = open_stage(&context, &claim).expect("open verified stage file");
    let mut raw = tempfile::tempfile().expect("temporary tar file");
    write_single_member(&mut reader, &mut raw, context.member_path())
        .expect("write one-file POSIX archive");
    raw.seek(SeekFrom::Start(0)).expect("rewind tar");
    let mut payload =
        scan_archive(&mut raw, context.member_path()).expect("read one-file POSIX archive");
    let mut actual = Vec::new();
    std::io::Read::read_to_end(&mut payload, &mut actual).expect("read payload");
    assert_eq!(actual, b"long path payload");
}

#[test]
fn rejects_foreign_paths_links_multiple_members_and_partial_trailing_data() {
    let expected = "../../runner-temp/stage.payload";
    let mut foreign = finish(member("../../outside", b'0', b"x"));
    assert!(scan_archive(&mut foreign, expected).is_err());

    let mut absolute = finish(member("/outside", b'0', b"x"));
    assert!(scan_archive(&mut absolute, expected).is_err());

    let mut link = finish(member(expected, b'2', b"target"));
    assert!(scan_archive(&mut link, expected).is_err());

    let mut device = finish(member(expected, b'3', b""));
    assert!(scan_archive(&mut device, expected).is_err());

    let mut unsupported_pax = member(
        "PaxHeaders/stage",
        b'x',
        &pax_record("linkpath", "../../outside"),
    );
    unsupported_pax.extend(member("stage", b'0', b"x"));
    let mut unsupported_pax = finish(unsupported_pax);
    assert!(scan_archive(&mut unsupported_pax, expected).is_err());

    let mut two_members = member("first", b'0', b"x");
    two_members.extend(member("second", b'0', b"y"));
    let mut multiple = finish(two_members);
    assert!(scan_archive(&mut multiple, "first").is_err());

    let mut partial = tempfile::tempfile().expect("temporary archive");
    partial
        .write_all(&member(expected, b'0', b"x"))
        .expect("write member");
    partial
        .write_all(&[0; BLOCK * 2])
        .expect("write end markers");
    partial.write_all(&[0]).expect("write incomplete block");
    partial.seek(SeekFrom::Start(0)).expect("rewind archive");
    assert!(scan_archive(&mut partial, expected).is_err());
}

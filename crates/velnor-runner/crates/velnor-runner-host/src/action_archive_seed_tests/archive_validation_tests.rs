use std::error::Error;
use std::fs;
use std::io::{self, Write};

use flate2::Compression;
use flate2::write::GzEncoder;
use tar::{Builder, EntryType, Header};

use super::*;

fn symlink_archive(links: &[(&str, &str)]) -> Result<Vec<u8>, Box<dyn Error>> {
    let encoder = GzEncoder::new(Vec::new(), Compression::default());
    let mut archive = Builder::new(encoder);
    for (name, target) in links {
        let mut header = Header::new_gnu();
        header.set_path(name)?;
        header.set_link_name(target)?;
        header.set_entry_type(EntryType::Symlink);
        header.set_size(0);
        header.set_mode(0o777);
        header.set_cksum();
        archive.append(&header, io::empty())?;
    }
    Ok(archive.into_inner()?.finish()?)
}

fn oversized_long_name_archive(size: usize) -> Result<Vec<u8>, Box<dyn Error>> {
    let body = vec![b'x'; size];
    let encoder = GzEncoder::new(Vec::new(), Compression::fast());
    let mut archive = Builder::new(encoder);
    let mut header = Header::new_gnu();
    header.set_path("././@LongLink")?;
    header.set_entry_type(EntryType::GNULongName);
    header.set_size(u64::try_from(body.len())?);
    header.set_mode(0o600);
    header.set_cksum();
    archive.append(&header, body.as_slice())?;
    Ok(archive.into_inner()?.finish()?)
}

fn pax_record(key: &str, value: &str) -> Vec<u8> {
    let field = format!("{key}={value}\n");
    let mut length = field.len() + 2;
    loop {
        let record = format!("{length} {field}");
        if record.len() == length {
            return record.into_bytes();
        }
        length = record.len();
    }
}

fn codeload_archive(commit_sha: &str, fields: &[(&str, &str)]) -> Result<Vec<u8>, Box<dyn Error>> {
    let body = fields
        .iter()
        .flat_map(|(key, value)| pax_record(key, value))
        .collect::<Vec<_>>();
    codeload_archive_with_body(commit_sha, &body)
}

fn codeload_archive_with_body(commit_sha: &str, body: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
    let encoder = GzEncoder::new(Vec::new(), Compression::default());
    let mut archive = Builder::new(encoder);
    let mut header = Header::new_gnu();
    header.set_path("pax_global_header")?;
    header.set_entry_type(EntryType::XGlobalHeader);
    header.set_size(u64::try_from(body.len())?);
    header.set_mode(0o644);
    header.set_cksum();
    archive.append(&header, body)?;

    let file_body = b"name: action\n";
    let mut action = Header::new_gnu();
    action.set_path(format!("checkout-{commit_sha}/action.yml"))?;
    action.set_size(u64::try_from(file_body.len())?);
    action.set_mode(0o644);
    action.set_cksum();
    archive.append(&action, file_body.as_slice())?;
    Ok(archive.into_inner()?.finish()?)
}

fn oversized_truncated_global_pax_archive(size: usize) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut header = Header::new_gnu();
    header.set_path("pax_global_header")?;
    header.set_entry_type(EntryType::XGlobalHeader);
    header.set_size(u64::try_from(size)?);
    header.set_mode(0o644);
    header.set_cksum();
    let mut tar_bytes = header.as_bytes().to_vec();
    tar_bytes.push(b'x');
    let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
    encoder.write_all(&tar_bytes)?;
    Ok(encoder.finish()?)
}

fn append_global_pax<W: Write>(
    archive: &mut Builder<W>,
    body: &[u8],
) -> Result<(), Box<dyn Error>> {
    let mut header = Header::new_gnu();
    header.set_path("pax_global_header")?;
    header.set_entry_type(EntryType::XGlobalHeader);
    header.set_size(u64::try_from(body.len())?);
    header.set_mode(0o644);
    header.set_cksum();
    archive.append(&header, body)?;
    Ok(())
}

fn global_pax_sequence(
    commit_sha: &str,
    repeated: bool,
    late: bool,
) -> Result<Vec<u8>, Box<dyn Error>> {
    let body = pax_record("comment", commit_sha);
    let encoder = GzEncoder::new(Vec::new(), Compression::default());
    let mut archive = Builder::new(encoder);
    if !late {
        append_global_pax(&mut archive, &body)?;
        if repeated {
            append_global_pax(&mut archive, &body)?;
        }
    }
    let file_body = b"name: action\n";
    let mut action = Header::new_gnu();
    action.set_path(format!("checkout-{commit_sha}/action.yml"))?;
    action.set_size(u64::try_from(file_body.len())?);
    action.set_mode(0o644);
    action.set_cksum();
    archive.append(&action, file_body.as_slice())?;
    if late {
        append_global_pax(&mut archive, &body)?;
    }
    Ok(archive.into_inner()?.finish()?)
}

fn write_archive(path: &Path, bytes: &[u8]) -> Result<u64, Box<dyn Error>> {
    fs::write(path, bytes)?;
    Ok(u64::try_from(bytes.len())?)
}

#[test]
fn resolves_links_after_removing_wrapper_and_rejects_escape_graphs() -> Result<(), Box<dyn Error>> {
    let root = TestRoot::new()?;
    let store = ActionArchiveStore::open(root.path().join("store"))?;
    let unsafe_links = [
        vec![("package/link", "../outside")],
        vec![("package/sub/b", "..")],
        vec![
            ("package/sub/a", "b/../../outside"),
            ("package/sub/b", ".."),
        ],
        vec![("package/one", "two"), ("package/two", "one")],
        vec![("package/link", "/outside")],
    ];
    for (index, links) in unsafe_links.iter().enumerate() {
        let bytes = symlink_archive(links)?;
        let action = format!("owner/action-{index}");
        let identity = identity(101, &action, &bytes);
        assert_eq!(
            store.publish(&identity, std::io::Cursor::new(bytes)).err(),
            Some(ActionArchiveSeedError::UnsafeEntry)
        );
    }
    Ok(())
}

#[test]
fn rejects_long_name_extension_before_reading_its_large_body() -> Result<(), Box<dyn Error>> {
    let root = TestRoot::new()?;
    let archive = oversized_long_name_archive(1024 * 1024)?;
    let path = root.path().join("oversized.tar.gz");
    let compressed_size = write_archive(&path, &archive)?;

    assert_eq!(
        crate::action_archive_seed::validate_archive_with_limit(
            &path,
            compressed_size,
            1024,
            "0123456789abcdef0123456789abcdef01234567",
        )
        .err(),
        Some(ActionArchiveSeedError::UnsafeEntry)
    );
    Ok(())
}

#[test]
fn accepts_codeload_global_comment_with_the_requested_commit() -> Result<(), Box<dyn Error>> {
    const COMMIT: &str = "11bd71901bbe5b1630ceea73d27597364c9af683";
    let root = TestRoot::new()?;
    let store = ActionArchiveStore::open(root.path().join("store"))?;
    let comment_record = pax_record("comment", COMMIT);
    assert_eq!(comment_record.len(), 52);
    let bytes = codeload_archive(COMMIT, &[("comment", COMMIT)])?;
    let mut action = identity(101, "actions/checkout", &bytes);
    action.commit_sha = COMMIT.to_owned();

    store.publish(&action, std::io::Cursor::new(bytes))?;
    Ok(())
}

#[test]
fn rejects_global_pax_path_linkpath_and_extra_fields() -> Result<(), Box<dyn Error>> {
    const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";
    let root = TestRoot::new()?;
    let store = ActionArchiveStore::open(root.path().join("store"))?;
    let unsafe_fields = [
        vec![("comment", COMMIT), ("path", "../../outside")],
        vec![("comment", COMMIT), ("linkpath", "/outside")],
        vec![("comment", COMMIT), ("unknown", "value")],
        vec![("comment", COMMIT), ("comment", COMMIT)],
        vec![("comment", "ffffffffffffffffffffffffffffffffffffffff")],
    ];
    for (index, fields) in unsafe_fields.iter().enumerate() {
        let bytes = codeload_archive(COMMIT, fields)?;
        let action = identity(101, &format!("actions/checkout-{index}"), &bytes);
        assert_eq!(
            store.publish(&action, std::io::Cursor::new(bytes)).err(),
            Some(ActionArchiveSeedError::UnsafeEntry)
        );
    }
    Ok(())
}

#[test]
fn rejects_oversized_global_pax_body_before_reading_it() -> Result<(), Box<dyn Error>> {
    let root = TestRoot::new()?;
    let store = ActionArchiveStore::open(root.path().join("store"))?;
    let bytes = oversized_truncated_global_pax_archive(4097)?;
    let action = identity(101, "actions/checkout", &bytes);

    assert_eq!(
        store.publish(&action, std::io::Cursor::new(bytes)).err(),
        Some(ActionArchiveSeedError::SizeLimit)
    );
    Ok(())
}

#[test]
fn rejects_malformed_repeated_and_late_global_pax_headers() -> Result<(), Box<dyn Error>> {
    const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";
    let root = TestRoot::new()?;
    let store = ActionArchiveStore::open(root.path().join("store"))?;
    let mut malformed = pax_record("comment", COMMIT);
    malformed[0] = b'9';
    let malformed_archive = codeload_archive_with_body(COMMIT, &malformed)?;
    let malformed_identity = identity(101, "actions/checkout-malformed", &malformed_archive);
    assert_eq!(
        store
            .publish(&malformed_identity, std::io::Cursor::new(malformed_archive))
            .err(),
        Some(ActionArchiveSeedError::UnsafeEntry)
    );

    for (index, (repeated, late)) in [(true, false), (false, true)].iter().enumerate() {
        let bytes = global_pax_sequence(COMMIT, *repeated, *late)?;
        let action = identity(101, &format!("actions/checkout-sequence-{index}"), &bytes);
        assert_eq!(
            store.publish(&action, std::io::Cursor::new(bytes)).err(),
            Some(ActionArchiveSeedError::UnsafeEntry)
        );
    }
    Ok(())
}

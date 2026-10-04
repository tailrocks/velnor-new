use std::error::Error;
use std::fs;
use std::io;

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
        crate::action_archive_seed::validate_archive_with_limit(&path, compressed_size, 1024).err(),
        Some(ActionArchiveSeedError::UnsafeEntry)
    );
    Ok(())
}

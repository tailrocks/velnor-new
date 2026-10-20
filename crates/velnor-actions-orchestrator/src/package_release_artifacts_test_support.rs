//! Synthetic package-output fixtures; no host executables are run.

use std::{
    fs,
    io::{Read, Write},
    path::PathBuf,
};

use flate2::write::GzEncoder;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tar::{Builder, Header};
use velnor_actions_contract::config::RustBinaryReleaseCompanion;
use velnor_actions_contract::{
    PackageReleasePlanProduct, package_release::PackageReleasePayloadFormat,
};

use super::{HANDOFF_SCHEMA, PackageOutputManifest};

pub(super) struct Fixture {
    _root: tempfile::TempDir,
    pub producer: PathBuf,
    pub staging: PathBuf,
    pub products: Vec<PackageReleasePlanProduct>,
    pub companions: Vec<RustBinaryReleaseCompanion>,
    pub commit: String,
    pub source_ref: String,
    pub version: String,
}

pub(super) struct Expected {
    pub payload_name: String,
    pub sbom_name: String,
}

pub(super) fn fixture() -> Fixture {
    let root = tempfile::tempdir().expect("temporary fixture root");
    let path = root.path().to_path_buf();
    let producer = path.join("producer");
    fs::create_dir(&producer).expect("producer directory");
    let staging = path.join("staging");
    Fixture {
        _root: root,
        producer,
        staging,
        products: vec![PackageReleasePlanProduct {
            id: "cli".to_owned(),
            package: "jackin".to_owned(),
            bins: vec!["cli".to_owned()],
            targets: vec!["x86_64-unknown-linux-gnu".to_owned()],
            format: PackageReleasePayloadFormat::TarGz,
        }],
        companions: vec![RustBinaryReleaseCompanion {
            id: "license".to_owned(),
            path: "licenses/LICENSE".to_owned(),
            asset_name: "LICENSE".to_owned(),
        }],
        commit: "a".repeat(40),
        source_ref: "refs/heads/main".to_owned(),
        version: "1.2.3-preview.7+abcdef0".to_owned(),
    }
}

pub(super) fn expected() -> Expected {
    let payload_name = "cli-x86_64-unknown-linux-gnu.tar.gz".to_owned();
    Expected {
        sbom_name: format!("{payload_name}.sbom.json"),
        payload_name,
    }
}

pub(super) fn write_source_files(fixture: &Fixture, arch: &str, version: &str) {
    let expected = expected();
    let archive_path = fixture.producer.join(&expected.payload_name);
    write_archive(&archive_path, &[("cli", arch)], true);
    let archive_digest = sha256_file(&archive_path);
    write_sbom(
        &fixture.producer.join(&expected.sbom_name),
        &expected.payload_name,
        &archive_digest,
    );
    let companion_path = fixture.producer.join(&fixture.companions[0].path);
    fs::create_dir_all(companion_path.parent().expect("companion parent"))
        .expect("companion directory");
    let companion_bytes = if fixture.companions[0]
        .asset_name
        .rsplit_once('.')
        .is_some_and(|(_, extension)| extension == "json")
    {
        serde_json::to_vec(&json!({
            "source_commit": fixture.commit,
            "source_ref": fixture.source_ref,
            "version": version,
        }))
        .expect("companion JSON")
    } else {
        b"license bytes".to_vec()
    };
    fs::write(&companion_path, companion_bytes).expect("write companion");
    let manifest = make_manifest(fixture, version);
    write_manifest(
        fixture,
        &serde_json::to_value(manifest).expect("manifest value"),
    );
}

pub(super) fn write_archive(path: &std::path::Path, entries: &[(&str, &str)], executable: bool) {
    let gzip = GzEncoder::new(Vec::new(), flate2::Compression::default());
    let mut archive = Builder::new(gzip);
    for (name, arch) in entries {
        let mut header = Header::new_gnu();
        let bytes = fake_executable(arch);
        if name.contains("..") {
            header.set_path("safe-bin").expect("safe initial path");
            let header_bytes = header.as_mut_bytes();
            header_bytes[..100].fill(0);
            header_bytes[..name.len()].copy_from_slice(name.as_bytes());
        } else {
            header.set_path(name).expect("tar path");
        }
        header.set_size(bytes.len() as u64);
        header.set_mode(if executable { 0o755 } else { 0o644 });
        header.set_cksum();
        archive
            .append(&header, bytes.as_slice())
            .expect("append tar member");
    }
    let gzip = archive.into_inner().expect("finish tar");
    fs::write(path, gzip.finish().expect("finish gzip")).expect("write archive");
}

pub(super) fn write_symlink_archive(path: &std::path::Path) {
    let gzip = GzEncoder::new(Vec::new(), flate2::Compression::default());
    let mut archive = Builder::new(gzip);
    let mut header = Header::new_gnu();
    header.set_entry_type(tar::EntryType::Symlink);
    header.set_path("cli").expect("symlink path");
    header
        .set_link_name("../../outside")
        .expect("symlink target");
    header.set_size(0);
    header.set_mode(0o777);
    header.set_cksum();
    archive
        .append(&header, std::io::empty())
        .expect("append symlink");
    let gzip = archive.into_inner().expect("finish tar");
    fs::write(path, gzip.finish().expect("finish gzip")).expect("write symlink archive");
}

pub(super) fn append_nonzero_tar_trailer(path: &std::path::Path) {
    let compressed = fs::read(path).expect("read gzip archive");
    let mut tar_bytes = Vec::new();
    flate2::read::GzDecoder::new(compressed.as_slice())
        .read_to_end(&mut tar_bytes)
        .expect("decompress archive");
    tar_bytes.extend_from_slice(b"hidden second tar data");
    let mut encoder = GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&tar_bytes).expect("write gzip input");
    fs::write(path, encoder.finish().expect("finish gzip")).expect("rewrite archive");
}

pub(super) fn append_second_gzip_member(path: &std::path::Path) {
    let mut compressed = fs::read(path).expect("read first gzip member");
    let mut encoder = GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder
        .write_all(b"second gzip member")
        .expect("write second member");
    compressed.extend(encoder.finish().expect("finish second member"));
    fs::write(path, compressed).expect("append second gzip member");
}

pub(super) fn corrupt_gzip_crc(path: &std::path::Path) {
    let mut compressed = fs::read(path).expect("read gzip archive");
    let crc_index = compressed.len().checked_sub(8).expect("gzip trailer");
    compressed[crc_index] ^= 0xff;
    fs::write(path, compressed).expect("corrupt gzip CRC");
}

pub(super) fn truncate_gzip_trailer(path: &std::path::Path) {
    let mut compressed = fs::read(path).expect("read gzip archive");
    compressed.truncate(compressed.len().saturating_sub(5));
    fs::write(path, compressed).expect("truncate gzip trailer");
}

pub(super) fn write_pax_archive(path: &std::path::Path) {
    let gzip = GzEncoder::new(Vec::new(), flate2::Compression::default());
    let mut archive = Builder::new(gzip);
    archive
        .append_pax_extensions([("comment", b"metadata".as_slice())])
        .expect("append PAX extension");
    append_fixture_executable(&mut archive, "cli");
    let gzip = archive.into_inner().expect("finish tar");
    fs::write(path, gzip.finish().expect("finish gzip")).expect("write PAX archive");
}

pub(super) fn write_gnu_longname_archive(path: &std::path::Path) {
    let gzip = GzEncoder::new(Vec::new(), flate2::Compression::default());
    let mut archive = Builder::new(gzip);
    let long_name = b"unexpected-long-name\0";
    let mut header = Header::new_gnu();
    header.set_entry_type(tar::EntryType::GNULongName);
    header.set_path("././@LongLink").expect("GNU longname path");
    header.set_size(long_name.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    archive
        .append(&header, long_name.as_slice())
        .expect("append GNU longname");
    append_fixture_executable(&mut archive, "cli");
    let gzip = archive.into_inner().expect("finish tar");
    fs::write(path, gzip.finish().expect("finish gzip")).expect("write GNU archive");
}

pub(super) fn write_archive_with_mode(path: &std::path::Path, mode: u32) {
    let gzip = GzEncoder::new(Vec::new(), flate2::Compression::default());
    let mut archive = Builder::new(gzip);
    let bytes = fake_executable("x86_64");
    let mut header = Header::new_gnu();
    header.set_path("cli").expect("archive path");
    header.set_size(bytes.len() as u64);
    header.set_mode(mode);
    header.set_cksum();
    archive
        .append(&header, bytes.as_slice())
        .expect("append archive member");
    let gzip = archive.into_inner().expect("finish tar");
    fs::write(path, gzip.finish().expect("finish gzip")).expect("write archive");
}

fn append_fixture_executable(archive: &mut Builder<GzEncoder<Vec<u8>>>, name: &str) {
    let bytes = fake_executable("x86_64");
    let mut header = Header::new_gnu();
    header.set_path(name).expect("archive path");
    header.set_size(bytes.len() as u64);
    header.set_mode(0o755);
    header.set_cksum();
    archive
        .append(&header, bytes.as_slice())
        .expect("append archive member");
}

pub(super) fn write_sbom(path: &std::path::Path, payload_name: &str, digest: &str) {
    let document = json!({
        "bomFormat": "CycloneDX",
        "specVersion": "1.6",
        "serialNumber": "urn:uuid:4b5c016e-4a60-4aaf-9d6c-5b0f381935a2",
        "version": 1,
        "metadata": {
            "component": {
                "type": "file",
                "name": payload_name,
                "hashes": [{"alg": "SHA-256", "content": digest}]
            }
        },
        "components": []
    });
    fs::write(path, serde_json::to_vec(&document).expect("SBOM bytes")).expect("write SBOM");
}

pub(super) fn write_manifest(fixture: &Fixture, value: &Value) {
    fs::write(
        fixture.producer.join("package-output.json"),
        serde_json::to_vec(value).expect("serialize package output"),
    )
    .expect("write package output");
}

pub(super) fn make_manifest(fixture: &Fixture, version: &str) -> PackageOutputManifest {
    let format = fixture.products[0].format;
    let target = &fixture.products[0].targets[0];
    let payload_name = match format {
        PackageReleasePayloadFormat::TarGz => format!("cli-{target}.tar.gz"),
    };
    let sbom_name = format!("{payload_name}.sbom.json");
    let archive_path = fixture.producer.join(&payload_name);
    let sbom_path = fixture.producer.join(&sbom_name);
    let companion_path = fixture.producer.join(&fixture.companions[0].path);
    PackageOutputManifest {
        schema: HANDOFF_SCHEMA.to_owned(),
        source_commit: fixture.commit.clone(),
        source_ref: fixture.source_ref.clone(),
        version: version.to_owned(),
        payloads: vec![super::PackageOutputPayload {
            product_id: "cli".to_owned(),
            target: "x86_64-unknown-linux-gnu".to_owned(),
            format,
            name: payload_name.clone(),
            sha256: sha256_file(&archive_path),
        }],
        companions: vec![super::PackageOutputCompanion {
            id: fixture.companions[0].id.clone(),
            path: fixture.companions[0].path.clone(),
            name: fixture.companions[0].asset_name.clone(),
            sha256: sha256_file(&companion_path),
        }],
        sboms: vec![super::PackageOutputSbom {
            payload_name,
            name: sbom_name,
            sha256: sha256_file(&sbom_path),
        }],
    }
}

pub(super) fn fake_executable(arch: &str) -> Vec<u8> {
    let mut bytes = vec![0u8; 64];
    bytes[..4].copy_from_slice(b"\x7fELF");
    bytes[4] = 2;
    bytes[5] = 1;
    bytes[6] = 1;
    bytes[16..18].copy_from_slice(&3u16.to_le_bytes());
    let machine = if arch == "aarch64" { 183u16 } else { 62u16 };
    bytes[18..20].copy_from_slice(&machine.to_le_bytes());
    bytes
}

fn sha256_file(path: &std::path::Path) -> String {
    let bytes = fs::read(path).expect("read for SHA-256");
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        write!(output, "{byte:02x}").expect("format SHA-256");
    }
    output
}

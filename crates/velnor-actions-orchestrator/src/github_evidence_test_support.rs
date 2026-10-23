//! Small real ZIP fixtures shared by evidence collection and import tests.

use std::io::{Cursor, Write};

pub(crate) fn archive() -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .start_file(
            "nested/report.json",
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored)
                .unix_permissions(0o100_644),
        )
        .expect("archive entry");
    writer.write_all(b"{\"attempt\":1}").expect("archive body");
    writer.finish().expect("archive").into_inner()
}

// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::Cursor;

use velnor_ci::image_candidate::layer::scan;

use super::fixture::{gzip, member, plain_layer, tar};

fn refused(data: &[u8], gzip: bool) -> String {
    scan(Cursor::new(data.to_vec()), gzip, 1 << 30, 1000)
        .err()
        .map_or_else(|| "ACCEPTED".to_owned(), |e| e.to_string())
}

#[test]
fn a_layer_is_hashed_over_its_uncompressed_bytes() {
    let layer = plain_layer();
    let direct = scan(Cursor::new(layer.clone()), false, 1 << 30, 10)
        .unwrap_or_else(|e| unreachable!("{e}"));
    let compressed =
        scan(Cursor::new(gzip(&layer)), true, 1 << 30, 10).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(direct.diff_id, compressed.diff_id);
    assert_eq!(direct.size, layer.len() as u64);
    assert_eq!(compressed.members, 1);
}

#[test]
fn a_gzip_layer_with_zero_padding_or_several_members_is_accepted() {
    let layer = plain_layer();
    let mut padded = gzip(&layer);
    padded.extend_from_slice(&[0; 8]);
    assert_eq!(refused(&padded, true), "ACCEPTED");
    let mut split = gzip(&layer[..512]);
    split.extend(gzip(&layer[512..]));
    assert_eq!(refused(&split, true), "ACCEPTED");
}

#[test]
fn a_gzip_layer_with_trailing_garbage_a_cut_end_or_a_bad_checksum_is_refused() {
    let layer = gzip(&plain_layer());
    let not_tar = "OCI layer is not a tar archive.";
    let mut garbage = layer.clone();
    garbage.extend_from_slice(b"junk");
    assert_eq!(refused(&garbage, true), not_tar);
    assert_eq!(refused(&layer[..layer.len() - 6], true), not_tar);
    let mut crc = layer.clone();
    let at = crc.len() - 8;
    crc[at] ^= 0xff;
    assert_eq!(refused(&crc, true), not_tar);
    assert_eq!(refused(&plain_layer(), true), not_tar);
}

#[test]
fn a_layer_that_is_not_a_tar_or_is_empty_is_refused() {
    assert_eq!(
        refused(&b"not a tar".repeat(100), false),
        "Tar archive has an incorrect member size."
    );
    assert_eq!(refused(b"", false), "OCI layer is not a tar archive.");
    assert_eq!(
        refused(&plain_layer()[..700], false),
        "OCI layer is not a tar archive."
    );
}

#[test]
fn a_layer_repeating_a_path_in_any_spelling_is_refused() {
    for second in ["a", "./a", "a/", "a//"] {
        let layer = tar(&[("a", b"x"), (second, b"y")]);
        assert_eq!(
            refused(&layer, false),
            "OCI layer repeats a path: a",
            "{second}"
        );
    }
}

#[test]
fn a_layer_path_that_is_absolute_or_climbs_is_refused() {
    for name in ["/etc/passwd", "../x", "a/../b", "a/.."] {
        assert_eq!(
            refused(&tar(&[(name, b"x")]), false),
            "OCI layer has a path that is not safe.",
            "{name}"
        );
    }
}

#[test]
fn a_layer_extension_header_over_the_limit_is_refused() {
    let mut layer = member("pax", &vec![b'a'; (1 << 20) + 1], b'x');
    layer.extend(tar(&[("a", b"x")]));
    assert_eq!(
        refused(&layer, false),
        "Tar extension header exceeds its size limit."
    );
}

#[test]
fn the_uncompressed_size_and_path_count_limits_stop_the_layer() {
    let layer = plain_layer();
    let over = scan(Cursor::new(layer.clone()), false, 1000, 10)
        .err()
        .map(|e| e.to_string());
    assert_eq!(
        over.as_deref(),
        Some("OCI image layers exceed their uncompressed size limit.")
    );
    let many = tar(&[("a", b"x"), ("b", b"y")]);
    let paths = scan(Cursor::new(many), false, 1 << 30, 1)
        .err()
        .map(|e| e.to_string());
    assert_eq!(paths.as_deref(), Some("OCI layer has too many paths."));
    let bomb = gzip(&vec![0u8; 8 << 20]);
    let capped = scan(Cursor::new(bomb), true, 1 << 20, 10)
        .err()
        .map(|e| e.to_string());
    assert_eq!(
        capped.as_deref(),
        Some("OCI image layers exceed their uncompressed size limit.")
    );
}

#[test]
fn a_member_with_a_size_field_that_is_not_a_number_is_refused() {
    let mut layer = plain_layer();
    layer[124..136].copy_from_slice(b"zzzzzzzzzzz\0");
    assert_eq!(
        refused(&layer, false),
        "Tar archive has an incorrect member size."
    );
}

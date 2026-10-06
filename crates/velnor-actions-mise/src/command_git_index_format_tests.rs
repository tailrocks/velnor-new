use super::validate;
use sha1::{Digest, Sha1};
use sha2::Sha256;

fn push_u16(data: &mut Vec<u8>, value: u16) {
    data.extend_from_slice(&value.to_be_bytes());
}

fn push_u32(data: &mut Vec<u8>, value: u32) {
    data.extend_from_slice(&value.to_be_bytes());
}

fn extension(signature: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut data = Vec::with_capacity(8 + payload.len());
    data.extend_from_slice(signature);
    let Ok(size) = u32::try_from(payload.len()) else {
        return Vec::new();
    };
    push_u32(&mut data, size);
    data.extend_from_slice(payload);
    data
}

fn finish_checksum(mut data: Vec<u8>, oid_width: usize) -> Vec<u8> {
    match oid_width {
        20 => {
            let digest = Sha1::digest(&data);
            data.extend_from_slice(digest.as_slice());
        }
        32 => {
            let digest = Sha256::digest(&data);
            data.extend_from_slice(digest.as_slice());
        }
        _ => {}
    }
    data
}

fn v2_index(oid_width: usize, paths: &[&[u8]], extensions: &[u8]) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(b"DIRC");
    push_u32(&mut data, 2);
    let Ok(count) = u32::try_from(paths.len()) else {
        return Vec::new();
    };
    push_u32(&mut data, count);
    for path in paths {
        let entry_start = data.len();
        data.extend([0; 40]);
        data[entry_start + 24..entry_start + 28].copy_from_slice(&0o100_644_u32.to_be_bytes());
        data.extend(std::iter::repeat_n(0, oid_width));
        let Ok(name_length) = u16::try_from(path.len()) else {
            return Vec::new();
        };
        push_u16(&mut data, name_length);
        data.extend_from_slice(path);
        data.push(0);
        let padding = (8 - ((data.len() - entry_start) % 8)) % 8;
        data.extend(std::iter::repeat_n(0, padding));
    }
    data.extend_from_slice(extensions);
    finish_checksum(data, oid_width)
}

fn v3_extended_index(extra: u16) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(b"DIRC");
    push_u32(&mut data, 3);
    push_u32(&mut data, 1);
    data.extend([0; 40]);
    data[36..40].copy_from_slice(&0o100_644_u32.to_be_bytes());
    data.extend([0; 20]);
    push_u16(&mut data, 0x4001);
    push_u16(&mut data, extra);
    let entry_start = data.len();
    data.extend([b'a', 0]);
    let padding = (8 - ((data.len() - entry_start) % 8)) % 8;
    data.extend(std::iter::repeat_n(0, padding));
    finish_checksum(data, 20)
}

fn v4_index(entries: &[(u8, &[u8])], extensions: &[u8]) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(b"DIRC");
    push_u32(&mut data, 4);
    let Ok(count) = u32::try_from(entries.len()) else {
        return Vec::new();
    };
    push_u32(&mut data, count);
    let mut previous = Vec::new();
    for (strip, suffix) in entries {
        let entry_start = data.len();
        data.extend([0; 40]);
        data[entry_start + 24..entry_start + 28].copy_from_slice(&0o100_644_u32.to_be_bytes());
        data.extend([0; 20]);
        let strip_byte = *strip;
        let strip = usize::from(strip_byte);
        let name_length = if strip <= previous.len() {
            previous.len() - strip + suffix.len()
        } else {
            suffix.len()
        };
        let Ok(name_length) = u16::try_from(name_length) else {
            return Vec::new();
        };
        push_u16(&mut data, name_length);
        data.push(strip_byte);
        data.extend_from_slice(suffix);
        data.push(0);
        if strip <= previous.len() {
            previous.truncate(previous.len() - strip);
            previous.extend_from_slice(suffix);
        }
    }
    data.extend_from_slice(extensions);
    finish_checksum(data, 20)
}

fn code(data: &[u8]) -> String {
    match validate(data) {
        Ok(_) => "ok".to_owned(),
        Err(error) => error.to_string(),
    }
}

#[test]
fn accepts_both_complete_oid_widths_and_zero_aligned_padding() {
    assert_eq!(code(&v2_index(20, &[b"a"], &[])), "ok");
    assert_eq!(code(&v2_index(32, &[b"a"], &[])), "ok");
}

#[test]
fn rejects_ambiguous_mixed_width_layout() {
    let mut data = b"DIRC\0\0\0\x02\0\0\0\0TEST\0\0\0\x04".to_vec();
    data.extend([0; 4]);
    data.extend([0; 20]);
    assert_eq!(code(&data), "git_index_oid_width_ambiguous");
}

#[test]
fn uppercase_optional_payload_is_not_scanned_for_link() {
    let ext = extension(b"TEST", b"link");
    assert_eq!(code(&v2_index(20, &[b"a"], &ext)), "ok");
}

#[test]
fn split_empty_replacement_gets_split_code() {
    let split = extension(b"link", &[]);
    assert_eq!(
        code(&v2_index(20, &[b""], &split)),
        "git_index_split_unsupported"
    );
    assert_eq!(code(&v2_index(20, &[b""], &[])), "git_index_invalid");
}

#[test]
fn semantic_width_failure_cannot_hide_split_extension() {
    let base = v2_index(20, &[b"a"], &extension(b"link", &[]));
    let mut data = base[..76].to_vec();
    data.extend([0; 8]);
    data.extend_from_slice(&base[76..]);
    data[73] = 2;
    assert_eq!(code(&data), "git_index_split_unsupported");
}

#[test]
fn rejects_unknown_lowercase_sdir() {
    let ext = extension(b"sdir", &[]);
    assert_eq!(code(&v2_index(20, &[b"a"], &ext)), "git_index_invalid");
}

#[test]
fn validates_eoie_end_and_rejects_nonterminal_or_wrong_offset() {
    let mut payload = Vec::new();
    push_u32(&mut payload, 76);
    payload.extend([0; 20]);
    let eoie = extension(b"EOIE", &payload);
    assert_eq!(code(&v2_index(20, &[b"a"], &eoie)), "ok");

    let mut wrong = payload;
    wrong[..4].copy_from_slice(&12u32.to_be_bytes());
    assert_eq!(
        code(&v2_index(20, &[b"a"], &extension(b"EOIE", &wrong))),
        "git_index_invalid"
    );

    let mut after = eoie;
    after.extend(extension(b"TEST", &[]));
    assert_eq!(code(&v2_index(20, &[b"a"], &after)), "git_index_invalid");
}

#[test]
fn validates_ieot_partition_and_rejects_zero_or_overlap() {
    let mut payload = Vec::new();
    push_u32(&mut payload, 1);
    push_u32(&mut payload, 12);
    push_u32(&mut payload, 1);
    push_u32(&mut payload, 76);
    push_u32(&mut payload, 1);
    let ieot = extension(b"IEOT", &payload);
    assert_eq!(code(&v2_index(20, &[b"a", b"b"], &ieot)), "ok");

    let mut zero = payload.clone();
    zero[8..12].copy_from_slice(&0u32.to_be_bytes());
    assert_eq!(
        code(&v2_index(20, &[b"a", b"b"], &extension(b"IEOT", &zero))),
        "git_index_invalid"
    );

    let mut overlap = payload;
    overlap[12..16].copy_from_slice(&12u32.to_be_bytes());
    assert_eq!(
        code(&v2_index(20, &[b"a", b"b"], &extension(b"IEOT", &overlap))),
        "git_index_invalid"
    );
}

#[test]
fn validates_v3_extra_flags_and_rejects_reserved_bits() {
    assert_eq!(code(&v3_extended_index(0x4000)), "ok");
    assert_eq!(code(&v3_extended_index(0x8000)), "git_index_invalid");
}

#[test]
fn validates_v4_prefix_and_rejects_bad_varints_or_prefixes() {
    assert_eq!(code(&v4_index(&[(0, b"foo"), (1, b"r")], &[])), "ok");
    assert_eq!(
        code(&v4_index(&[(0, b"foo"), (4, b"x")], &[])),
        "git_index_invalid"
    );

    let mut overflow = b"DIRC\0\0\0\x04\0\0\0\x01".to_vec();
    overflow.extend([0; 40]);
    overflow.extend([0; 20]);
    push_u16(&mut overflow, 1);
    overflow.extend(std::iter::repeat_n(0x80, 10));
    overflow.extend([0, b'x']);
    overflow = finish_checksum(overflow, 20);
    assert_eq!(code(&overflow), "git_index_invalid");
}

#[test]
fn refuses_v4_ieot_block_resets_before_native_read() {
    let mut payload = Vec::new();
    push_u32(&mut payload, 1);
    push_u32(&mut payload, 12);
    push_u32(&mut payload, 1);
    push_u32(&mut payload, 79);
    push_u32(&mut payload, 1);
    let ieot = extension(b"IEOT", &payload);
    assert_eq!(
        code(&v4_index(&[(0, b"foo"), (3, b"bar")], &ieot)),
        "git_index_invalid"
    );
    assert_eq!(
        code(&v4_index(&[(0, b"foo"), (1, b"r")], &ieot)),
        "git_index_invalid"
    );
}

#[test]
fn encoded_v4_name_cursor_cannot_hide_native_split_extension() {
    let split = extension(b"link", &[]);
    assert_eq!(
        code(&v4_index(&[(0, b"a\0b")], &split)),
        "git_index_split_unsupported"
    );
    assert_eq!(code(&v4_index(&[(7, b"foo")], &[])), "ok");
}

#[test]
fn gitlink_type_bits_are_refused_even_with_extra_mode_bits() {
    for mode in [0o160_000_u32, 0o160_001] {
        let base = v2_index(20, &[b"submodule"], &[]);
        let mut data = base[..base.len() - 20].to_vec();
        data[36..40].copy_from_slice(&mode.to_be_bytes());
        assert_eq!(code(&finish_checksum(data, 20)), "git_index_invalid");
    }
}

#[test]
fn rejects_nonzero_v2_padding_and_name_length_mismatch() {
    let mut padding = v2_index(20, &[b"dir/a"], &[]);
    padding[80] = 1;
    assert_eq!(code(&padding), "git_index_invalid");

    let mut name = v2_index(20, &[b"a"], &[]);
    name[73] = 2;
    assert_eq!(code(&name), "git_index_invalid");
}

#[test]
fn rejects_tampered_trailing_checksum() {
    let mut data = v2_index(20, &[b"a"], &[]);
    let last = data.len() - 1;
    data[last] ^= 1;
    assert_eq!(code(&data), "git_index_checksum_mismatch");
}

use super::profile::Profile;

const PAX_HEADER_LIMIT: u64 = 64 * 1024;
const PAX_TOTAL_LIMIT: u64 = 16 * 1024 * 1024;

pub(super) fn check_header(profile: Profile, size: u64, metadata: &mut u64) -> Result<(), String> {
    if !profile.allow_pax {
        return Err("PAX tar extensions are forbidden".to_owned());
    }
    check_metadata_size(size, metadata)
}

pub(super) fn check_metadata_size(size: u64, metadata: &mut u64) -> Result<(), String> {
    if size > PAX_HEADER_LIMIT {
        return Err("tar metadata limit exceeded before allocation".to_owned());
    }
    *metadata = metadata
        .checked_add(size)
        .ok_or_else(|| "tar metadata size overflow".to_owned())?;
    if *metadata > PAX_TOTAL_LIMIT {
        return Err("tar metadata total limit exceeded".to_owned());
    }
    Ok(())
}

pub(super) fn parse(body: &[u8]) -> Result<(), String> {
    let mut offset = 0_usize;
    let mut records = 0_usize;
    while offset < body.len() {
        let space = record_length_boundary(body, offset)?;
        let length = parse_record_length(body, offset, space)?;
        let end = offset
            .checked_add(length)
            .ok_or_else(|| "PAX record length overflow".to_owned())?;
        if length <= space - offset + 1 || end > body.len() || body[end - 1] != b'\n' {
            return Err("malformed PAX record boundary".to_owned());
        }
        let (key, value) = record_fields(&body[space + 1..end - 1])?;
        check_key_value(key, value)?;
        records += 1;
        offset = end;
    }
    if records == 0 {
        return Err("empty PAX metadata".to_owned());
    }
    Ok(())
}

fn record_length_boundary(body: &[u8], offset: usize) -> Result<usize, String> {
    let space = body[offset..]
        .iter()
        .position(|byte| *byte == b' ')
        .map(|index| offset + index)
        .ok_or_else(|| "malformed PAX record length".to_owned())?;
    if space == offset || !body[offset..space].iter().all(u8::is_ascii_digit) {
        return Err("malformed PAX record length".to_owned());
    }
    Ok(space)
}

fn parse_record_length(body: &[u8], offset: usize, space: usize) -> Result<usize, String> {
    std::str::from_utf8(&body[offset..space])
        .map_err(|_| "PAX record length is not ASCII".to_owned())?
        .parse::<usize>()
        .map_err(|_| "PAX record length overflow".to_owned())
}

fn record_fields(record: &[u8]) -> Result<(&str, &str), String> {
    let equals = record
        .iter()
        .position(|byte| *byte == b'=')
        .ok_or_else(|| "PAX record lacks a value".to_owned())?;
    let key =
        std::str::from_utf8(&record[..equals]).map_err(|_| "PAX key is not UTF-8".to_owned())?;
    let value = std::str::from_utf8(&record[equals + 1..])
        .map_err(|_| "PAX value is not UTF-8".to_owned())?;
    Ok((key, value))
}

fn check_key_value(key: &str, value: &str) -> Result<(), String> {
    let forbidden = key.is_empty()
        || value.contains('\0')
        || key == "size"
        || key.starts_with("GNU.sparse.")
        || key == "GNU.dumpdir"
        || key.starts_with("SCHILY.");
    if forbidden {
        return Err("unsafe PAX key or value".to_owned());
    }
    Ok(())
}

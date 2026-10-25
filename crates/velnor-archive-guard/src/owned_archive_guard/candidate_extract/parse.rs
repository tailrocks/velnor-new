use super::{BLOCK, MEMBER_LIMIT, Member, TRAILER_LIMIT};

pub(super) fn parse_candidate(bytes: &[u8], expected: [&str; 3]) -> Result<[Member; 3], String> {
    let mut offset = 0usize;
    let mut members = Vec::with_capacity(3);
    let mut total = 0u64;
    for (index, expected_name) in expected.iter().enumerate() {
        let header_end = offset
            .checked_add(BLOCK)
            .ok_or_else(|| "candidate header bounds overflow".to_owned())?;
        let header = bytes
            .get(offset..header_end)
            .ok_or_else(|| "candidate archive contains fewer than three members".to_owned())?;
        let name = parse_member_name(header)?;
        if name != *expected_name {
            return Err("candidate archive members differ from expected order".to_owned());
        }
        if !matches!(header[156], 0 | b'0') {
            return Err("candidate archive contains a non-regular member".to_owned());
        }
        let mode = parse_octal(&header[100..108])?;
        let required_mode = if index == 0 { 0o755 } else { 0o644 };
        if mode != required_mode {
            return Err("candidate archive member mode differs from policy".to_owned());
        }
        let size = parse_octal(&header[124..136])?;
        if size > MEMBER_LIMIT {
            return Err("candidate archive member exceeds size limit".to_owned());
        }
        total = total
            .checked_add(size)
            .ok_or_else(|| "candidate archive total size overflow".to_owned())?;
        if total > MEMBER_LIMIT {
            return Err("candidate archive exceeds total size limit".to_owned());
        }
        let data_start = header_end;
        let data_len = usize::try_from(size)
            .map_err(|_| "candidate member size conversion failed".to_owned())?;
        let padded_len = data_len
            .checked_add(BLOCK - 1)
            .ok_or_else(|| "candidate member padding overflow".to_owned())?
            / BLOCK
            * BLOCK;
        offset = data_start
            .checked_add(padded_len)
            .ok_or_else(|| "candidate member bounds overflow".to_owned())?;
        if offset > bytes.len() {
            return Err("candidate archive member is truncated".to_owned());
        }
        members.push(Member {
            name,
            data_start,
            size: data_len,
        });
    }
    if members.len() != 3 || members.iter().any(|member| member.name.is_empty()) {
        return Err("unexpected candidate archive member count".to_owned());
    }
    let trailer = bytes
        .get(offset..)
        .ok_or_else(|| "candidate archive end marker is unavailable".to_owned())?;
    if !(1024..=TRAILER_LIMIT).contains(&trailer.len())
        || trailer.len() % BLOCK != 0
        || trailer.iter().any(|byte| *byte != 0)
    {
        return Err("candidate archive has an invalid end marker".to_owned());
    }
    members
        .try_into()
        .map_err(|_| "unexpected candidate archive member count".to_owned())
}

pub(super) fn parse_octal(field: &[u8]) -> Result<u64, String> {
    let mut value = 0u64;
    let mut started = false;
    let mut ended = false;
    for byte in field {
        if *byte == 0 || *byte == b' ' {
            ended |= started;
        } else {
            if ended || !(b'0'..=b'7').contains(byte) {
                return Err("candidate archive contains invalid octal metadata".to_owned());
            }
            started = true;
            value = value
                .checked_mul(8)
                .and_then(|current| current.checked_add(u64::from(*byte - b'0')))
                .ok_or_else(|| "candidate archive octal metadata overflow".to_owned())?;
        }
    }
    Ok(value)
}

pub(super) fn validate_names(expected: [&str; 3]) -> Result<(), String> {
    for name in expected {
        if name.is_empty()
            || name == "."
            || name == ".."
            || name.contains('/')
            || name.contains('\\')
            || name.as_bytes().contains(&0)
        {
            return Err("candidate output name is unsafe".to_owned());
        }
    }
    if expected[0] == expected[1] || expected[0] == expected[2] || expected[1] == expected[2] {
        return Err("candidate output names are duplicated".to_owned());
    }
    Ok(())
}

fn parse_member_name(header: &[u8]) -> Result<String, String> {
    let raw_name = &header[..100];
    let name_length = raw_name.iter().position(|byte| *byte == 0).unwrap_or(100);
    let raw_prefix = &header[345..500];
    let prefix_length = raw_prefix.iter().position(|byte| *byte == 0).unwrap_or(155);
    let name_part = std::str::from_utf8(&raw_name[..name_length])
        .map_err(|_| "candidate archive member name is not UTF-8".to_owned())?;
    let prefix = std::str::from_utf8(&raw_prefix[..prefix_length])
        .map_err(|_| "candidate archive member prefix is not UTF-8".to_owned())?;
    let name = if prefix.is_empty() {
        name_part.to_owned()
    } else {
        format!("{prefix}/{name_part}")
    };
    Ok(name)
}

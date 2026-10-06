//! Bounded path decoding shared by the finite Git index parser.
pub(super) fn read_ofs(data: &[u8], cursor: &mut usize, limit: usize) -> Option<usize> {
    if *cursor >= limit {
        return None;
    }
    let mut byte = *data.get(*cursor)?;
    *cursor = cursor.checked_add(1)?;
    let mut value = usize::from(byte & 0x7f);
    while byte & 0x80 != 0 {
        if *cursor >= limit {
            return None;
        }
        byte = *data.get(*cursor)?;
        *cursor = cursor.checked_add(1)?;
        value = value
            .checked_add(1)?
            .checked_mul(128)?
            .checked_add(usize::from(byte & 0x7f))?;
        if *cursor > limit {
            return None;
        }
    }
    Some(value)
}

pub(super) fn read_nul_range(data: &[u8], cursor: usize, limit: usize) -> Option<(usize, usize)> {
    let tail = data.get(cursor..limit)?;
    let length = tail.iter().position(|byte| *byte == 0)?;
    let end = cursor.checked_add(length)?;
    Some((end, end.checked_add(1)?))
}

pub(super) fn valid_name_length(encoded: usize, actual: usize) -> bool {
    if encoded == 0x0fff {
        actual >= 0x0fff
    } else {
        actual == encoded
    }
}

pub(super) fn valid_path_range(data: &[u8], start: usize, end: usize, name_length: usize) -> bool {
    data.get(start..end)
        .is_some_and(|path| valid_name_length(name_length, path.len()) && valid_path(path))
}

pub(super) fn valid_path(path: &[u8]) -> bool {
    if path.is_empty() || path.contains(&0) {
        return false;
    }
    if path.first() == Some(&b'/') || path.last() == Some(&b'/') {
        return false;
    }
    let mut component_start = 0;
    for (index, byte) in path.iter().enumerate() {
        if *byte != b'/' {
            continue;
        }
        if invalid_component(&path[component_start..index]) {
            return false;
        }
        component_start = index + 1;
    }
    !invalid_component(&path[component_start..])
}

fn invalid_component(component: &[u8]) -> bool {
    component.is_empty() || matches!(component, b"." | b".." | b".git")
}

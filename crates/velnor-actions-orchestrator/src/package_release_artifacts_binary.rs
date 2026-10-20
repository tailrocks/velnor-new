pub(super) fn is_expected_executable(bytes: &[u8], target: &str) -> bool {
    if target.contains("apple-darwin") {
        return mach_o_arch(bytes).is_some_and(|arch| target.starts_with(arch));
    }
    if target.contains("linux") {
        return elf_arch(bytes).is_some_and(|arch| target.starts_with(arch));
    }
    false
}

fn elf_arch(bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() < 20 || bytes.get(..4)? != b"\x7fELF" || bytes.get(4)? != &2 {
        return None;
    }
    let endian = *bytes.get(5)?;
    let machine = match endian {
        1 => u16::from_le_bytes([*bytes.get(18)?, *bytes.get(19)?]),
        2 => u16::from_be_bytes([*bytes.get(18)?, *bytes.get(19)?]),
        _ => return None,
    };
    let file_type = match endian {
        1 => u16::from_le_bytes([*bytes.get(16)?, *bytes.get(17)?]),
        2 => u16::from_be_bytes([*bytes.get(16)?, *bytes.get(17)?]),
        _ => return None,
    };
    if !matches!(file_type, 2 | 3) {
        return None;
    }
    match machine {
        183 => Some("aarch64"),
        62 => Some("x86_64"),
        _ => None,
    }
}

fn mach_o_arch(bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() < 8 {
        return None;
    }
    let (little_endian, magic) = match bytes.get(..4)? {
        [0xcf, 0xfa, 0xed, 0xfe] => (true, true),
        [0xfe, 0xed, 0xfa, 0xcf] => (false, true),
        _ => (false, false),
    };
    if !magic {
        return None;
    }
    let raw = [
        *bytes.get(4)?,
        *bytes.get(5)?,
        *bytes.get(6)?,
        *bytes.get(7)?,
    ];
    let cpu = if little_endian {
        u32::from_le_bytes(raw)
    } else {
        u32::from_be_bytes(raw)
    };
    let file_type_bytes = [
        *bytes.get(12)?,
        *bytes.get(13)?,
        *bytes.get(14)?,
        *bytes.get(15)?,
    ];
    let file_type = if little_endian {
        u32::from_le_bytes(file_type_bytes)
    } else {
        u32::from_be_bytes(file_type_bytes)
    };
    if file_type != 2 {
        return None;
    }
    match cpu {
        0x0100_000c => Some("aarch64"),
        0x0100_0007 => Some("x86_64"),
        _ => None,
    }
}

// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt;

const ELF_MAGIC: [u8; 4] = [0x7f, b'E', b'L', b'F'];
const CLASS_64: u8 = 2;
const LITTLE_ENDIAN: u8 = 1;
const MACHINE_X86_64: u16 = 62;
const PT_LOAD: u32 = 1;
const PT_DYNAMIC: u32 = 2;
const PT_INTERP: u32 = 3;
const DT_NULL: u64 = 0;
const DT_NEEDED: u64 = 1;
const DT_STRTAB: u64 = 5;
const DT_RPATH: u64 = 15;
const DT_RUNPATH: u64 = 29;
const DT_VERNEED: u64 = 0x6fff_fffe;
const DT_VERNEEDNUM: u64 = 0x6fff_ffff;
const MAX_ENTRIES: usize = 65_536;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElfError {
    NotElf,
    Not64BitLittleEndian,
    Truncated(&'static str),
    Malformed(&'static str),
}

impl fmt::Display for ElfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotElf => f.write_str("not an ELF file"),
            Self::Not64BitLittleEndian => f.write_str("not a 64-bit little-endian ELF file"),
            Self::Truncated(part) => write!(f, "truncated ELF file: {part} is cut short"),
            Self::Malformed(part) => write!(f, "malformed ELF file: {part}"),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ElfFacts {
    pub x86_64: bool,
    pub dynamic: bool,
    pub needed: Vec<String>,
    pub library_paths: Vec<String>,
    pub glibc: Option<(u32, u32)>,
}

struct Reader<'a> {
    bytes: &'a [u8],
}

impl Reader<'_> {
    fn slice(&self, offset: u64, length: u64, part: &'static str) -> Result<&[u8], ElfError> {
        let start = usize::try_from(offset).map_err(|_| ElfError::Truncated(part))?;
        let end = offset
            .checked_add(length)
            .and_then(|end| usize::try_from(end).ok())
            .ok_or(ElfError::Truncated(part))?;
        self.bytes.get(start..end).ok_or(ElfError::Truncated(part))
    }

    fn u16(&self, offset: u64, part: &'static str) -> Result<u16, ElfError> {
        let bytes = self.slice(offset, 2, part)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    fn u32(&self, offset: u64, part: &'static str) -> Result<u32, ElfError> {
        let bytes = self.slice(offset, 4, part)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn u64(&self, offset: u64, part: &'static str) -> Result<u64, ElfError> {
        let bytes = self.slice(offset, 8, part)?;
        let mut word = [0; 8];
        word.copy_from_slice(bytes);
        Ok(u64::from_le_bytes(word))
    }

    fn text(&self, offset: u64, part: &'static str) -> Result<String, ElfError> {
        let start = usize::try_from(offset).map_err(|_| ElfError::Truncated(part))?;
        let rest = self.bytes.get(start..).ok_or(ElfError::Truncated(part))?;
        let end = rest
            .iter()
            .position(|byte| *byte == 0)
            .ok_or(ElfError::Truncated(part))?;
        String::from_utf8(rest[..end].to_vec()).map_err(|_| ElfError::Malformed(part))
    }
}

struct Segment {
    kind: u32,
    offset: u64,
    address: u64,
    file_size: u64,
}

fn segments(reader: &Reader<'_>) -> Result<Vec<Segment>, ElfError> {
    let table = reader.u64(0x20, "program header offset")?;
    let size = u64::from(reader.u16(0x36, "program header size")?);
    let count = u64::from(reader.u16(0x38, "program header count")?);
    if size < 56 {
        return Err(ElfError::Malformed("program header size"));
    }
    (0..count)
        .map(|index| {
            let at = table.saturating_add(index * size);
            Ok(Segment {
                kind: reader.u32(at, "program header")?,
                offset: reader.u64(at + 8, "program header")?,
                address: reader.u64(at + 16, "program header")?,
                file_size: reader.u64(at + 32, "program header")?,
            })
        })
        .collect()
}

fn file_offset(loads: &[&Segment], address: u64) -> Result<u64, ElfError> {
    loads
        .iter()
        .find(|load| address >= load.address && address - load.address < load.file_size)
        .map(|load| load.offset.saturating_add(address - load.address))
        .ok_or(ElfError::Malformed(
            "an address outside every loaded segment",
        ))
}

fn glibc_version(name: &str) -> Option<(u32, u32)> {
    let rest = name.strip_prefix("GLIBC_")?;
    let mut parts = rest.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    Some((major, minor))
}

fn needs(
    reader: &Reader<'_>,
    strings: u64,
    mut at: u64,
    count: u64,
) -> Result<Option<(u32, u32)>, ElfError> {
    let mut highest = None;
    for _ in 0..count {
        let aux_count = reader.u16(at.saturating_add(2), "version need")?;
        let mut aux =
            at.saturating_add(u64::from(reader.u32(at.saturating_add(8), "version need")?));
        for _ in 0..aux_count {
            let name = reader.text(
                strings.saturating_add(u64::from(
                    reader.u32(aux.saturating_add(8), "version name")?,
                )),
                "version name",
            )?;
            highest = highest.max(glibc_version(&name));
            aux = aux.saturating_add(u64::from(
                reader.u32(aux.saturating_add(12), "version name")?,
            ));
        }
        let next = u64::from(reader.u32(at.saturating_add(12), "version need")?);
        if next == 0 {
            break;
        }
        at = at.saturating_add(next);
    }
    Ok(highest)
}

fn dynamic_entries(reader: &Reader<'_>, segment: &Segment) -> Result<Vec<(u64, u64)>, ElfError> {
    let mut entries = Vec::new();
    for index in 0..segment.file_size / 16 {
        let at = segment.offset.saturating_add(index * 16);
        let tag = reader.u64(at, "dynamic section")?;
        if tag == DT_NULL {
            break;
        }
        if entries.len() >= MAX_ENTRIES {
            return Err(ElfError::Malformed("dynamic section is too long"));
        }
        entries.push((tag, reader.u64(at + 8, "dynamic section")?));
    }
    Ok(entries)
}

pub fn read(bytes: &[u8]) -> Result<ElfFacts, ElfError> {
    if bytes.get(..4) != Some(&ELF_MAGIC) {
        return Err(ElfError::NotElf);
    }
    if bytes.get(4) != Some(&CLASS_64) || bytes.get(5) != Some(&LITTLE_ENDIAN) {
        return Err(ElfError::Not64BitLittleEndian);
    }
    let reader = Reader { bytes };
    let mut facts = ElfFacts {
        x86_64: reader.u16(0x12, "machine")? == MACHINE_X86_64,
        ..ElfFacts::default()
    };
    let segments = segments(&reader)?;
    facts.dynamic = segments.iter().any(|segment| segment.kind == PT_INTERP);
    let Some(dynamic) = segments.iter().find(|segment| segment.kind == PT_DYNAMIC) else {
        return Ok(facts);
    };
    let loads: Vec<&Segment> = segments
        .iter()
        .filter(|segment| segment.kind == PT_LOAD)
        .collect();
    let entries = dynamic_entries(&reader, dynamic)?;
    let value = |wanted: u64| {
        entries
            .iter()
            .find(|(tag, _)| *tag == wanted)
            .map(|(_, value)| *value)
    };
    let strings = file_offset(
        &loads,
        value(DT_STRTAB).ok_or(ElfError::Malformed("no strtab"))?,
    )?;
    for (tag, offset) in &entries {
        let name = |part| reader.text(strings.saturating_add(*offset), part);
        match *tag {
            DT_NEEDED => facts.needed.push(name("needed library")?),
            DT_RPATH | DT_RUNPATH => facts.library_paths.extend(
                name("library path")?
                    .split(':')
                    .filter(|path| !path.is_empty())
                    .map(str::to_owned),
            ),
            _ => {}
        }
    }
    if let (Some(table), Some(count)) = (value(DT_VERNEED), value(DT_VERNEEDNUM)) {
        facts.glibc = needs(&reader, strings, file_offset(&loads, table)?, count)?;
    }
    Ok(facts)
}

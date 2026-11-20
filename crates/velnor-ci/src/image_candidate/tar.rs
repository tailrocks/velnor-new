// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::{self, Read};

pub const EXTENSION_LIMIT: u64 = 1 << 20;
pub const EXTENSION_TOTAL_LIMIT: u64 = 4 << 20;
const BLOCK: usize = 512;
const CHUNK: usize = 1 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    File,
    Dir,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: Vec<u8>,
    pub kind: Kind,
    pub size: u64,
    pub offset: u64,
}

#[derive(Debug)]
pub enum TarError {
    Format(&'static str),
    ExtensionSize,
    ExtensionTotal,
    MemberSize,
    Io(io::Error),
}

impl From<io::Error> for TarError {
    fn from(error: io::Error) -> Self {
        if error.kind() == io::ErrorKind::UnexpectedEof {
            Self::Format("truncated archive")
        } else {
            Self::Io(error)
        }
    }
}

#[derive(Debug)]
pub struct TarReader<R> {
    inner: R,
    pos: u64,
    data_left: u64,
    padding: u64,
    extension_total: u64,
    buffer: Vec<u8>,
}

impl<R: Read> TarReader<R> {
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            pos: 0,
            data_left: 0,
            padding: 0,
            extension_total: 0,
            buffer: Vec::new(),
        }
    }

    #[must_use]
    pub fn position(&self) -> u64 {
        self.pos
    }

    pub fn into_inner(self) -> R {
        self.inner
    }

    pub fn read_data(&mut self, sink: &mut dyn FnMut(&[u8])) -> Result<(), TarError> {
        if self.buffer.is_empty() {
            self.buffer = vec![0; CHUNK];
        }
        while self.data_left > 0 {
            let want = usize::try_from(self.data_left.min(CHUNK as u64)).unwrap_or(CHUNK);
            let got = self.inner.read(&mut self.buffer[..want])?;
            if got == 0 {
                return Err(TarError::Format("truncated archive"));
            }
            self.pos += got as u64;
            self.data_left -= got as u64;
            sink(&self.buffer[..got]);
        }
        Ok(())
    }

    fn discard(&mut self, mut count: u64) -> Result<(), TarError> {
        if self.buffer.is_empty() {
            self.buffer = vec![0; CHUNK];
        }
        while count > 0 {
            let want = usize::try_from(count.min(CHUNK as u64)).unwrap_or(CHUNK);
            let got = self.inner.read(&mut self.buffer[..want])?;
            if got == 0 {
                return Err(TarError::Format("truncated archive"));
            }
            self.pos += got as u64;
            count -= got as u64;
        }
        Ok(())
    }

    fn read_block(&mut self) -> Result<Option<[u8; BLOCK]>, TarError> {
        let mut block = [0u8; BLOCK];
        let mut filled = 0;
        while filled < BLOCK {
            let got = self.inner.read(&mut block[filled..])?;
            if got == 0 {
                break;
            }
            filled += got;
        }
        self.pos += filled as u64;
        match filled {
            0 => Ok(None),
            BLOCK => Ok(Some(block)),
            _ => Err(TarError::Format("truncated header")),
        }
    }

    fn extension_data(&mut self, size: u64) -> Result<Vec<u8>, TarError> {
        if size > EXTENSION_LIMIT {
            return Err(TarError::ExtensionSize);
        }
        self.extension_total += size;
        if self.extension_total > EXTENSION_TOTAL_LIMIT {
            return Err(TarError::ExtensionTotal);
        }
        let mut data = Vec::new();
        self.data_left = size;
        self.padding = pad(size);
        self.read_data(&mut |chunk| data.extend_from_slice(chunk))?;
        let padding = self.padding;
        self.padding = 0;
        self.discard(padding)?;
        Ok(data)
    }

    pub fn next_entry(&mut self) -> Result<Option<Entry>, TarError> {
        let rest = self.data_left + self.padding;
        self.data_left = 0;
        self.padding = 0;
        self.discard(rest)?;
        let mut gnu_name: Option<Vec<u8>> = None;
        let mut pax_name: Option<Vec<u8>> = None;
        loop {
            let Some(block) = self.read_block()? else {
                return Ok(None);
            };
            if block.iter().all(|byte| *byte == 0) {
                return Ok(None);
            }
            let size = member_size(&block)?;
            check_sum(&block)?;
            check_numeric_fields(&block)?;
            let kind = block[156];
            match kind {
                b'x' | b'L' => {
                    let data = self.extension_data(size)?;
                    if kind == b'x' {
                        if let Some(path) = pax_path(&data)? {
                            pax_name = Some(path);
                        }
                    } else {
                        gnu_name = Some(cstr(&data).to_vec());
                    }
                }
                b'K' => {
                    self.extension_data(size)?;
                }
                b'g' => {
                    self.extension_data(size)?;
                    return Err(TarError::Format("global extended header"));
                }
                b'S' => return Err(TarError::Format("sparse member")),
                _ => {
                    self.data_left = size;
                    self.padding = pad(size);
                    return Ok(Some(entry(
                        &block,
                        kind,
                        size,
                        pax_name.or(gnu_name),
                        self.pos,
                    )));
                }
            }
        }
    }
}

fn entry(block: &[u8; BLOCK], kind: u8, size: u64, name: Option<Vec<u8>>, offset: u64) -> Entry {
    let mut base = cstr(&block[0..100]).to_vec();
    let prefix = cstr(&block[345..500]);
    if !prefix.is_empty() {
        let mut joined = prefix.to_vec();
        joined.push(b'/');
        joined.extend_from_slice(&base);
        base = joined;
    }
    let directory = kind == b'5' || (kind == 0 && base.ends_with(b"/"));
    let mut name = name.unwrap_or(base);
    if directory {
        while name.ends_with(b"/") {
            name.pop();
        }
    }
    let kind = match kind {
        _ if directory => Kind::Dir,
        0 | b'0' | b'7' => Kind::File,
        _ => Kind::Other,
    };
    Entry {
        name,
        kind,
        size,
        offset,
    }
}

fn cstr(field: &[u8]) -> &[u8] {
    field.split(|byte| *byte == 0).next().unwrap_or_default()
}

fn pad(size: u64) -> u64 {
    size.div_ceil(BLOCK as u64) * BLOCK as u64 - size
}

fn check_sum(block: &[u8; BLOCK]) -> Result<(), TarError> {
    let stored = parse_octal(&block[148..156]).ok_or(TarError::Format("bad checksum"))?;
    let unsigned: u64 = block
        .iter()
        .enumerate()
        .map(|(index, byte)| {
            if (148..156).contains(&index) {
                32
            } else {
                u64::from(*byte)
            }
        })
        .sum();
    let signed: i64 = block
        .iter()
        .enumerate()
        .map(|(index, byte)| {
            if (148..156).contains(&index) {
                32
            } else {
                i64::from(i8::from_ne_bytes([*byte]))
            }
        })
        .sum();
    if stored == unsigned || i64::try_from(stored).is_ok_and(|value| value == signed) {
        Ok(())
    } else {
        Err(TarError::Format("bad checksum"))
    }
}

fn parse_octal(field: &[u8]) -> Option<u64> {
    let start = field.iter().position(|byte| !matches!(byte, b' ' | 0));
    let end = field.iter().rposition(|byte| !matches!(byte, b' ' | 0));
    let trimmed = match (start, end) {
        (Some(start), Some(end)) => &field[start..=end],
        _ => &[],
    };
    if trimmed.is_empty() {
        return Some(0);
    }
    trimmed.iter().try_fold(0u64, |value, byte| match byte {
        b'0'..=b'7' => value.checked_mul(8)?.checked_add(u64::from(byte - b'0')),
        _ => None,
    })
}

fn member_size(block: &[u8; BLOCK]) -> Result<u64, TarError> {
    let field = &block[124..136];
    if field[0] & 0x80 != 0 {
        let mut value: u128 = 0;
        for (index, byte) in field.iter().enumerate() {
            let part = if index == 0 { byte & 0x7f } else { *byte };
            value = (value << 8) | u128::from(part);
        }
        return i64::try_from(value)
            .map(i64::unsigned_abs)
            .map_err(|_| TarError::MemberSize);
    }
    parse_octal(field).ok_or(TarError::MemberSize)
}

fn check_numeric_fields(block: &[u8; BLOCK]) -> Result<(), TarError> {
    let ustar = block[257..262] == *b"ustar";
    let fields: &[(usize, usize)] = if ustar {
        &[
            (100, 108),
            (108, 116),
            (116, 124),
            (136, 148),
            (329, 337),
            (337, 345),
        ]
    } else {
        &[(100, 108), (108, 116), (116, 124), (136, 148)]
    };
    for &(from, to) in fields {
        let field = &block[from..to];
        if field[0] & 0x80 == 0 && parse_octal(field).is_none() {
            return Err(TarError::Format("numeric header field"));
        }
    }
    Ok(())
}

fn pax_path(data: &[u8]) -> Result<Option<Vec<u8>>, TarError> {
    let mut rest = data;
    let mut path = None;
    while !rest.is_empty() {
        let space = rest
            .iter()
            .position(|byte| *byte == b' ')
            .ok_or(TarError::Format("pax record"))?;
        let digits =
            std::str::from_utf8(&rest[..space]).map_err(|_| TarError::Format("pax record"))?;
        if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(TarError::Format("pax record"));
        }
        let length: usize = digits.parse().map_err(|_| TarError::Format("pax record"))?;
        if length <= space + 1 || length > rest.len() || rest[length - 1] != b'\n' {
            return Err(TarError::Format("pax record"));
        }
        let record = &rest[space + 1..length - 1];
        let equals = record
            .iter()
            .position(|byte| *byte == b'=')
            .ok_or(TarError::Format("pax record"))?;
        let (key, value) = (&record[..equals], &record[equals + 1..]);
        if key.is_empty() {
            return Err(TarError::Format("pax record"));
        }
        if key == b"size" || key.starts_with(b"GNU.sparse") {
            return Err(TarError::Format("pax size or sparse record"));
        }
        if key == b"path" {
            path = Some(value.to_vec());
        }
        rest = &rest[length..];
    }
    Ok(path)
}

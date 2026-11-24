// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub(super) struct Elf {
    pub(super) machine: u16,
    pub(super) interpreter: bool,
    pub(super) needed: Vec<&'static str>,
    pub(super) versions: Vec<&'static str>,
    pub(super) library_path: Option<&'static str>,
}

impl Elf {
    pub(super) fn dynamic_x86_64() -> Self {
        Self {
            machine: 62,
            interpreter: true,
            needed: vec!["libc.so.6"],
            versions: vec!["GLIBC_2.17", "GLIBC_2.34", "GLIBC_PRIVATE"],
            library_path: None,
        }
    }

    pub(super) fn needing(mut self, library: &'static str) -> Self {
        self.needed.push(library);
        self
    }

    pub(super) fn with_versions(mut self, versions: &[&'static str]) -> Self {
        self.versions = versions.to_vec();
        self
    }

    pub(super) fn with_library_path(mut self, path: &'static str) -> Self {
        self.library_path = Some(path);
        self
    }

    pub(super) fn aarch64(mut self) -> Self {
        self.machine = 183;
        self
    }

    pub(super) fn without_interpreter(mut self) -> Self {
        self.interpreter = false;
        self
    }
}

fn narrow(value: u64) -> u32 {
    u32::try_from(value).unwrap_or(0)
}

fn word(bytes: &mut Vec<u8>, value: u64) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

struct Strings(Vec<u8>);

impl Strings {
    fn add(&mut self, text: &str) -> u64 {
        let at = self.0.len() as u64;
        self.0.extend_from_slice(text.as_bytes());
        self.0.push(0);
        at
    }
}

fn header(elf: &Elf, headers: u16) -> Vec<u8> {
    let mut bytes = vec![0x7f, b'E', b'L', b'F', 2, 1, 1];
    bytes.resize(16, 0);
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&elf.machine.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    word(&mut bytes, 0);
    word(&mut bytes, 64);
    word(&mut bytes, 0);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    for field in [64u16, 56, headers, 64, 0, 0] {
        bytes.extend_from_slice(&field.to_le_bytes());
    }
    bytes
}

fn program_header(kind: u32, offset: u64, size: u64) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&kind.to_le_bytes());
    bytes.extend_from_slice(&5u32.to_le_bytes());
    for value in [offset, offset, offset, size, size, 8] {
        word(&mut bytes, value);
    }
    bytes
}

pub(super) fn build(elf: &Elf) -> Vec<u8> {
    let headers: u16 = if elf.interpreter { 3 } else { 2 };
    let interpreter = b"/lib64/ld-linux-x86-64.so.2\0";
    let mut strings = Strings(vec![0]);
    let needed: Vec<u64> = elf.needed.iter().map(|name| strings.add(name)).collect();
    let path = elf.library_path.map(|text| strings.add(text));
    let file = strings.add("libc.so.6");
    let versions: Vec<u64> = elf.versions.iter().map(|name| strings.add(name)).collect();
    let table = 64 + 56 * u64::from(headers) + interpreter.len() as u64;
    let strtab = table.next_multiple_of(8);
    let verneed = (strtab + strings.0.len() as u64).next_multiple_of(8);
    let mut need = Vec::new();
    need.extend_from_slice(&1u16.to_le_bytes());
    need.extend_from_slice(&u16::try_from(versions.len()).unwrap_or(0).to_le_bytes());
    need.extend_from_slice(&narrow(file).to_le_bytes());
    need.extend_from_slice(&16u32.to_le_bytes());
    need.extend_from_slice(&0u32.to_le_bytes());
    for (index, name) in versions.iter().enumerate() {
        need.extend_from_slice(&0u32.to_le_bytes());
        need.extend_from_slice(&0u16.to_le_bytes());
        need.extend_from_slice(&0u16.to_le_bytes());
        need.extend_from_slice(&narrow(*name).to_le_bytes());
        let next: u32 = if index + 1 == versions.len() { 0 } else { 16 };
        need.extend_from_slice(&next.to_le_bytes());
    }
    let dynamic = (verneed + need.len() as u64).next_multiple_of(8);
    let mut entries: Vec<(u64, u64)> = needed.iter().map(|name| (1, *name)).collect();
    entries.extend(path.map(|at| (29, at)));
    entries.extend([
        (5, strtab),
        (0x6fff_fffe, verneed),
        (0x6fff_ffff, 1),
        (0, 0),
    ]);
    let size = dynamic + 16 * entries.len() as u64;
    let mut bytes = header(elf, headers);
    bytes.extend(program_header(1, 0, size));
    if elf.interpreter {
        bytes.extend(program_header(
            3,
            64 + 56 * u64::from(headers),
            interpreter.len() as u64,
        ));
    }
    bytes.extend(program_header(2, dynamic, 16 * entries.len() as u64));
    bytes.extend_from_slice(interpreter);
    bytes.resize(usize::try_from(strtab).unwrap_or(0), 0);
    bytes.extend_from_slice(&strings.0);
    bytes.resize(usize::try_from(verneed).unwrap_or(0), 0);
    bytes.extend_from_slice(&need);
    bytes.resize(usize::try_from(dynamic).unwrap_or(0), 0);
    for (tag, value) in entries {
        word(&mut bytes, tag);
        word(&mut bytes, value);
    }
    bytes
}

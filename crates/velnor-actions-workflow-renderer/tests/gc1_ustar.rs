// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::io::Write as _;
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};

const BLOCK: usize = 512;
const ZERO_CHUNK: usize = 1024 * 1024;

pub(crate) struct Archive {
    child: Child,
    stdin: ChildStdin,
}

fn octal(field: &mut [u8], value: u64) {
    let digits = format!("{value:0width$o}", width = field.len() - 1);
    field[..digits.len()].copy_from_slice(digits.as_bytes());
}

fn header(name: &str, typeflag: u8, size: u64, link: &str, mode: u64) -> [u8; BLOCK] {
    let mut block = [0u8; BLOCK];
    block[..name.len()].copy_from_slice(name.as_bytes());
    octal(&mut block[100..108], mode);
    octal(&mut block[108..116], 0);
    octal(&mut block[116..124], 0);
    octal(&mut block[124..136], size);
    octal(&mut block[136..148], 0);
    block[148..156].fill(b' ');
    block[156] = typeflag;
    block[157..157 + link.len()].copy_from_slice(link.as_bytes());
    block[257..263].copy_from_slice(b"ustar\0");
    block[263..265].copy_from_slice(b"00");
    let sum: u32 = block.iter().map(|byte| u32::from(*byte)).sum();
    let digits = format!("{sum:06o}");
    block[148..154].copy_from_slice(digits.as_bytes());
    block[154] = 0;
    block[155] = b' ';
    block
}

impl Archive {
    pub(crate) fn create(out: &Path) -> Result<Self, Box<dyn Error>> {
        let mut child = Command::new("gzip")
            .arg("-c")
            .stdin(Stdio::piped())
            .stdout(std::fs::File::create(out)?)
            .spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| std::io::Error::other("gzip stdin unavailable"))?;
        Ok(Self { child, stdin })
    }

    pub(crate) fn dir(&mut self, name: &str) -> Result<(), Box<dyn Error>> {
        self.stdin.write_all(&header(name, b'5', 0, "", 0o755))?;
        Ok(())
    }

    pub(crate) fn locked_dir(&mut self, name: &str) -> Result<(), Box<dyn Error>> {
        self.stdin.write_all(&header(name, b'5', 0, "", 0))?;
        Ok(())
    }

    pub(crate) fn file(&mut self, name: &str, content: &[u8]) -> Result<(), Box<dyn Error>> {
        self.stdin
            .write_all(&header(name, b'0', content.len() as u64, "", 0o644))?;
        self.stdin.write_all(content)?;
        self.stdin.write_all(&vec![
            0u8;
            content.len().next_multiple_of(BLOCK) - content.len()
        ])?;
        Ok(())
    }

    pub(crate) fn symlink(&mut self, name: &str, target: &str) -> Result<(), Box<dyn Error>> {
        self.stdin
            .write_all(&header(name, b'2', 0, target, 0o644))?;
        Ok(())
    }

    pub(crate) fn hardlink(&mut self, name: &str, target: &str) -> Result<(), Box<dyn Error>> {
        self.stdin
            .write_all(&header(name, b'1', 0, target, 0o644))?;
        Ok(())
    }

    pub(crate) fn zeros(&mut self, name: &str, size: u64) -> Result<(), Box<dyn Error>> {
        self.stdin.write_all(&header(name, b'0', size, "", 0o644))?;
        let chunk = vec![0u8; ZERO_CHUNK];
        let mut left = size.next_multiple_of(BLOCK as u64);
        while left > 0 {
            let take = usize::try_from(left).map_or(ZERO_CHUNK, |left| left.min(ZERO_CHUNK));
            self.stdin.write_all(&chunk[..take])?;
            left -= u64::try_from(take)?;
        }
        Ok(())
    }

    pub(crate) fn state_layout(&mut self, mark: &str) -> Result<(), Box<dyn Error>> {
        self.dir("state/")?;
        self.dir("state/action-results/")?;
        self.file("state/action-results/result", b"velnor")?;
        self.dir("state/cas/")?;
        self.file("state/cas/object", mark.as_bytes())?;
        self.file("state/mbx-cache-export-v1.json", b"{\"version\":1}\n")
    }

    pub(crate) fn finish(mut self) -> Result<(), Box<dyn Error>> {
        self.stdin.write_all(&[0u8; BLOCK * 2])?;
        drop(self.stdin);
        if self.child.wait()?.success() {
            Ok(())
        } else {
            Err(std::io::Error::other("gzip failed").into())
        }
    }
}

pub(crate) fn gzip_again(from: &Path, to: &Path) -> Result<(), Box<dyn Error>> {
    let status = Command::new("gzip")
        .arg("-c")
        .arg(from)
        .stdout(std::fs::File::create(to)?)
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other("gzip failed").into())
    }
}

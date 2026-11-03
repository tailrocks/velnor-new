// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::io::Write as _;
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};

const BLOCK: usize = 512;
const ZERO_CHUNK: usize = 1024 * 1024;

const GZIP_FLAGS: usize = 3;
const GZIP_HEADER: usize = 10;
const GZIP_COMMENT: u8 = 0x10;
const USTAR_OFFSET: usize = 257;
const NAME_FIELD: usize = 100;
const LINK_FIELD: usize = 100;
const LONG_NAME_ENTRY: &str = "././@LongLink";

pub(crate) struct Archive {
    child: Child,
    stdin: ChildStdin,
    plain_bytes: u64,
    content_bytes: u64,
}

fn octal(field: &mut [u8], value: u64) {
    let digits = format!("{value:0width$o}", width = field.len() - 1);
    field[..digits.len()].copy_from_slice(digits.as_bytes());
}

fn header(
    name: &str,
    typeflag: u8,
    size: u64,
    link: &str,
    mode: u64,
) -> Result<[u8; BLOCK], Box<dyn Error>> {
    if name.len() > NAME_FIELD || link.len() > LINK_FIELD {
        return Err(std::io::Error::other(format!(
            "tar header field over its limit: {} name bytes, {} link bytes",
            name.len(),
            link.len()
        ))
        .into());
    }
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
    Ok(block)
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
        Ok(Self {
            child,
            stdin,
            plain_bytes: 0,
            content_bytes: 0,
        })
    }

    pub(crate) fn content_bytes(&self) -> u64 {
        self.content_bytes
    }

    fn put(&mut self, bytes: &[u8]) -> Result<(), Box<dyn Error>> {
        self.stdin.write_all(bytes)?;
        self.plain_bytes += u64::try_from(bytes.len())?;
        Ok(())
    }

    fn entry(
        &mut self,
        name: &str,
        typeflag: u8,
        size: u64,
        link: &str,
        mode: u64,
    ) -> Result<(), Box<dyn Error>> {
        if name.len() > NAME_FIELD {
            self.long_record(b'L', name)?;
        }
        if link.len() > LINK_FIELD {
            self.long_record(b'K', link)?;
        }
        let short_name = &name[..name.len().min(NAME_FIELD)];
        let short_link = &link[..link.len().min(LINK_FIELD)];
        self.put(&header(short_name, typeflag, size, short_link, mode)?)
    }

    fn long_record(&mut self, typeflag: u8, text: &str) -> Result<(), Box<dyn Error>> {
        let mut record = text.as_bytes().to_vec();
        record.push(0);
        let padded = record.len().next_multiple_of(BLOCK) - record.len();
        self.put(&header(
            LONG_NAME_ENTRY,
            typeflag,
            record.len() as u64,
            "",
            0,
        )?)?;
        self.put(&record)?;
        self.put(&vec![0u8; padded])
    }

    pub(crate) fn dir(&mut self, name: &str) -> Result<(), Box<dyn Error>> {
        self.dir_mode(name, 0o755)
    }

    pub(crate) fn locked_dir(&mut self, name: &str) -> Result<(), Box<dyn Error>> {
        self.dir_mode(name, 0)
    }

    pub(crate) fn dir_mode(&mut self, name: &str, mode: u64) -> Result<(), Box<dyn Error>> {
        self.entry(name, b'5', 0, "", mode)
    }

    pub(crate) fn file(&mut self, name: &str, content: &[u8]) -> Result<(), Box<dyn Error>> {
        self.entry(name, b'0', content.len() as u64, "", 0o644)?;
        self.put(content)?;
        self.put(&vec![
            0u8;
            content.len().next_multiple_of(BLOCK) - content.len()
        ])?;
        self.content_bytes += content.len() as u64;
        Ok(())
    }

    pub(crate) fn symlink(&mut self, name: &str, target: &str) -> Result<(), Box<dyn Error>> {
        self.entry(name, b'2', 0, target, 0o644)
    }

    pub(crate) fn hardlink(&mut self, name: &str, target: &str) -> Result<(), Box<dyn Error>> {
        self.entry(name, b'1', 0, target, 0o644)
    }

    pub(crate) fn fifo(&mut self, name: &str) -> Result<(), Box<dyn Error>> {
        self.entry(name, b'6', 0, "", 0o644)
    }

    pub(crate) fn zeros(&mut self, name: &str, size: u64) -> Result<(), Box<dyn Error>> {
        self.entry(name, b'0', size, "", 0o644)?;
        let chunk = vec![0u8; ZERO_CHUNK];
        let mut left = size.next_multiple_of(BLOCK as u64);
        while left > 0 {
            let take = usize::try_from(left).map_or(ZERO_CHUNK, |left| left.min(ZERO_CHUNK));
            self.put(&chunk[..take])?;
            left -= u64::try_from(take)?;
        }
        self.content_bytes += size;
        Ok(())
    }

    pub(crate) fn raw_zeros(&mut self, size: u64) -> Result<(), Box<dyn Error>> {
        let chunk = vec![0u8; ZERO_CHUNK];
        let mut left = size;
        while left > 0 {
            let take = usize::try_from(left).map_or(ZERO_CHUNK, |left| left.min(ZERO_CHUNK));
            self.put(&chunk[..take])?;
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

    pub(crate) fn finish(mut self) -> Result<u64, Box<dyn Error>> {
        self.put(&[0u8; BLOCK * 2])?;
        self.finish_untrailed()
    }

    pub(crate) fn finish_untrailed(self) -> Result<u64, Box<dyn Error>> {
        let Self {
            mut child,
            stdin,
            plain_bytes,
            ..
        } = self;
        drop(stdin);
        if child.wait()?.success() {
            Ok(plain_bytes)
        } else {
            Err(std::io::Error::other("gzip failed").into())
        }
    }
}

pub(crate) fn polyglot(from: &Path, to: &Path) -> Result<(), Box<dyn Error>> {
    let mut bytes = std::fs::read(from)?;
    if bytes.get(GZIP_FLAGS) != Some(&0) {
        return Err(std::io::Error::other("fixture gzip already carries header fields").into());
    }
    bytes[GZIP_FLAGS] = GZIP_COMMENT;
    let mut comment = vec![b'A'; USTAR_OFFSET - GZIP_HEADER];
    comment.extend_from_slice(b"ustar\0");
    bytes.splice(GZIP_HEADER..GZIP_HEADER, comment);
    std::fs::write(to, bytes)?;
    Ok(())
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

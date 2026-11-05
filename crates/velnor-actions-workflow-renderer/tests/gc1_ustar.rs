// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::io::Write as _;
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};

const BLOCK: usize = 512;
const ZERO_CHUNK: usize = 1024 * 1024;
const BEYOND_CALENDAR_MTIME: u64 = 99_999_999_999_999_999;

const SKIPPABLE_MAGIC: [u8; 4] = [0x50, 0x2a, 0x4d, 0x18];
const SKIPPABLE_HEADER: usize = 8;
const LONG_WINDOW_FRAME: [u8; 9] = [0x28, 0xb5, 0x2f, 0xfd, 0x00, 0xa8, 0x01, 0x00, 0x00];
const USTAR_OFFSET: usize = 257;
const NAME_FIELD: usize = 100;
const LINK_FIELD: usize = 100;
const LONG_NAME_ENTRY: &str = "././@LongLink";

pub(crate) struct Archive {
    child: Child,
    stdin: ChildStdin,
    plain_bytes: u64,
    ustar: bool,
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
    ustar: bool,
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
    block[156] = typeflag;
    block[157..157 + link.len()].copy_from_slice(link.as_bytes());
    if ustar {
        block[257..263].copy_from_slice(b"ustar\0");
        block[263..265].copy_from_slice(b"00");
    }
    seal(&mut block);
    Ok(block)
}

fn seal(block: &mut [u8; BLOCK]) {
    block[148..156].fill(b' ');
    let sum: u32 = block.iter().map(|byte| u32::from(*byte)).sum();
    let digits = format!("{sum:06o}");
    block[148..154].copy_from_slice(digits.as_bytes());
    block[154] = 0;
    block[155] = b' ';
}

#[derive(Clone, Copy)]
enum Codec {
    Zstd,
    Gzip,
}

impl Archive {
    pub(crate) fn create(out: &Path) -> Result<Self, Box<dyn Error>> {
        Self::spawn(out, Codec::Zstd)
    }

    pub(crate) fn create_v7(out: &Path) -> Result<Self, Box<dyn Error>> {
        let mut archive = Self::spawn(out, Codec::Zstd)?;
        archive.ustar = false;
        Ok(archive)
    }

    pub(crate) fn create_gzip(out: &Path) -> Result<Self, Box<dyn Error>> {
        Self::spawn(out, Codec::Gzip)
    }

    fn spawn(out: &Path, codec: Codec) -> Result<Self, Box<dyn Error>> {
        let mut command = match codec {
            Codec::Zstd => {
                let mut command = Command::new("zstd");
                command.args(["-3", "--long=30", "-q", "-c"]);
                command
            }
            Codec::Gzip => {
                let mut command = Command::new("gzip");
                command.arg("-c");
                command
            }
        };
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(std::fs::File::create(out)?)
            .spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| std::io::Error::other("compressor stdin unavailable"))?;
        Ok(Self {
            child,
            stdin,
            plain_bytes: 0,
            ustar: true,
        })
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
        self.put(&header(
            short_name, typeflag, size, short_link, mode, self.ustar,
        )?)
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
            self.ustar,
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
        Ok(())
    }

    pub(crate) fn file_beyond_calendar(
        &mut self,
        name: &str,
        content: &[u8],
    ) -> Result<(), Box<dyn Error>> {
        let mut block = header(name, b'0', content.len() as u64, "", 0o644, self.ustar)?;
        block[136] = 0x80;
        block[137..148].copy_from_slice(&u128::from(BEYOND_CALENDAR_MTIME).to_be_bytes()[5..16]);
        seal(&mut block);
        self.put(&block)?;
        self.put(content)?;
        self.put(&vec![
            0u8;
            content.len().next_multiple_of(BLOCK) - content.len()
        ])
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
            Err(std::io::Error::other("compressor failed").into())
        }
    }
}

fn skippable_frame(data: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut frame = SKIPPABLE_MAGIC.to_vec();
    frame.extend_from_slice(&u32::try_from(data.len())?.to_le_bytes());
    frame.extend_from_slice(data);
    Ok(frame)
}

fn ustar_bait() -> Vec<u8> {
    let mut data = vec![b'A'; USTAR_OFFSET - SKIPPABLE_HEADER];
    data.extend_from_slice(b"ustar\0");
    data
}

pub(crate) fn skippable_prefix(from: &Path, to: &Path) -> Result<(), Box<dyn Error>> {
    let mut bytes = skippable_frame(&ustar_bait())?;
    bytes.extend(std::fs::read(from)?);
    std::fs::write(to, bytes)?;
    Ok(())
}

pub(crate) fn skippable_only(to: &Path) -> Result<(), Box<dyn Error>> {
    std::fs::write(to, skippable_frame(&ustar_bait())?)?;
    Ok(())
}

pub(crate) fn long_window_frame(to: &Path) -> Result<(), Box<dyn Error>> {
    std::fs::write(to, LONG_WINDOW_FRAME)?;
    Ok(())
}

pub(crate) fn truncate_to(from: &Path, to: &Path, keep: usize) -> Result<(), Box<dyn Error>> {
    let bytes = std::fs::read(from)?;
    std::fs::write(to, &bytes[..keep.min(bytes.len())])?;
    Ok(())
}

pub(crate) fn pad_to(file: &Path, exact_len: u64) -> Result<(), Box<dyn Error>> {
    let current = std::fs::metadata(file)?.len();
    if current == exact_len {
        return Ok(());
    }
    let room = exact_len
        .checked_sub(current + SKIPPABLE_HEADER as u64)
        .ok_or_else(|| std::io::Error::other("payload cannot be padded to that length"))?;
    let frame = skippable_frame(&vec![0u8; usize::try_from(room)?])?;
    let mut out = std::fs::OpenOptions::new().append(true).open(file)?;
    out.write_all(&frame)?;
    Ok(())
}

fn pipe_through(
    program: &str,
    args: &[&str],
    from: &Path,
    to: &Path,
) -> Result<(), Box<dyn Error>> {
    let status = Command::new(program)
        .args(args)
        .stdin(std::fs::File::open(from)?)
        .stdout(std::fs::File::create(to)?)
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!("{program} failed")).into())
    }
}

pub(crate) fn wrap_zstd(from: &Path, to: &Path) -> Result<(), Box<dyn Error>> {
    pipe_through("zstd", &["-3", "--long=30", "-q", "-c"], from, to)
}

pub(crate) fn decode_zstd(payload: &Path, to: &Path) -> Result<(), Box<dyn Error>> {
    pipe_through("zstd", &["-dc", "--long=30", "-q"], payload, to)
}

pub(crate) fn list_members(payload: &Path) -> Result<String, Box<dyn Error>> {
    let plain = payload.with_extension("plain-list");
    decode_zstd(payload, &plain)?;
    let output = Command::new("tar")
        .arg("-tf")
        .arg("-")
        .stdin(std::fs::File::open(&plain)?)
        .output()?;
    std::fs::remove_file(&plain)?;
    if !output.status.success() {
        return Err(std::io::Error::other("the system tar cannot list the fixture").into());
    }
    Ok(String::from_utf8(output.stdout)?)
}

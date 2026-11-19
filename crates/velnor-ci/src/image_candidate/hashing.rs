// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::{self, Read};

use sha2::{Digest, Sha256};

#[derive(Debug)]
pub struct Hashing<R> {
    inner: R,
    sha: Sha256,
    size: u64,
    limit: u64,
}

impl<R: Read> Hashing<R> {
    pub fn new(inner: R, limit: u64) -> Self {
        Self {
            inner,
            sha: Sha256::new(),
            size: 0,
            limit,
        }
    }

    pub fn into_parts(self) -> (R, String, u64) {
        (self.inner, hex(&self.sha.finalize()), self.size)
    }

    pub fn finish(self) -> (String, u64) {
        (hex(&self.sha.finalize()), self.size)
    }
}

impl<R: Read> Read for Hashing<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let count = self.inner.read(buf)?;
        self.size += count as u64;
        if self.size > self.limit {
            return Err(io::Error::new(io::ErrorKind::FileTooLarge, "size limit"));
        }
        self.sha.update(&buf[..count]);
        Ok(count)
    }
}

#[must_use]
pub fn is_limit(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::FileTooLarge
}

#[must_use]
pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    bytes
        .iter()
        .flat_map(|byte| {
            [
                DIGITS[usize::from(byte >> 4)],
                DIGITS[usize::from(byte & 15)],
            ]
        })
        .map(char::from)
        .collect()
}

#[must_use]
pub fn sha256_hex(data: &[u8]) -> String {
    hex(&Sha256::digest(data))
}

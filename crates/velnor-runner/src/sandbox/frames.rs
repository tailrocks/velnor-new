// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::Read;

use anyhow::{Context, Result, ensure};
use zeroize::Zeroizing;

pub(super) const FRAME_LIMIT: usize = 64 * 1024;

pub(super) struct Delivery {
    pub(super) jit: Zeroizing<Vec<u8>>,
    pub(super) trusted: Zeroizing<Vec<u8>>,
}

fn frame(source: &mut impl Read, what: &str) -> Result<Zeroizing<Vec<u8>>> {
    let mut prefix = [0_u8; 4];
    source
        .read_exact(&mut prefix)
        .with_context(|| format!("read the length of the {what} frame from standard input"))?;
    let length = usize::try_from(u32::from_be_bytes(prefix)).unwrap_or(usize::MAX);
    ensure!(
        (1..=FRAME_LIMIT).contains(&length),
        "the {what} frame is empty or larger than {FRAME_LIMIT} bytes"
    );
    let mut bytes = Zeroizing::new(vec![0_u8; length]);
    source
        .read_exact(&mut bytes)
        .with_context(|| format!("read the {what} frame from standard input"))?;
    Ok(bytes)
}

impl Delivery {
    pub(super) fn read(mut source: impl Read) -> Result<Self> {
        Ok(Self {
            jit: frame(&mut source, "JIT")?,
            trusted: frame(&mut source, "trusted facts")?,
        })
    }
}

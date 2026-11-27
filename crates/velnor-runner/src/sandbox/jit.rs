// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::{PipeReader, Read, Write};

use anyhow::{Context, Result, ensure};
use zeroize::Zeroizing;

const LIMIT: u64 = 60 * 1024;

pub(super) struct Jit(Zeroizing<Vec<u8>>);

impl Jit {
    pub(super) fn read(source: impl Read) -> Result<Self> {
        let mut bytes = Zeroizing::new(Vec::new());
        source
            .take(LIMIT + 1)
            .read_to_end(&mut bytes)
            .context("read the JIT configuration from standard input")?;
        ensure!(
            !bytes.is_empty() && u64::try_from(bytes.len()).is_ok_and(|length| length <= LIMIT),
            "the JIT configuration on standard input is empty or larger than {LIMIT} bytes"
        );
        Ok(Self(bytes))
    }

    pub(super) fn pipe(self) -> Result<PipeReader> {
        let (reader, mut writer) = std::io::pipe().context("create the JIT pipe")?;
        std::thread::Builder::new()
            .name("jit-writer".to_owned())
            .spawn(move || {
                let _ = writer.write_all(&self.0);
            })
            .context("start the JIT writer")?;
        Ok(reader)
    }
}

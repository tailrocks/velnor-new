// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::{PipeReader, Write};

use anyhow::{Context, Result};
use zeroize::Zeroizing;

pub(super) struct Jit(pub(super) Zeroizing<Vec<u8>>);

impl Jit {
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

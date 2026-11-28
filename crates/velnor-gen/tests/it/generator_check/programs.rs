// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::world::{QUIET, put, runs_every, skips_every, world};
use crate::facts::TestResult;

const PROGRAMS: [&str; 12] = [
    "[build]\nrustc-wrapper = \"tools/wrap.rs\"\n",
    "[build]\nrustc-workspace-wrapper = \"tools/wrap.rs\"\n",
    "[build]\nrustc = \"tools/rustc.rs\"\n",
    "[build]\nrustdoc = \"tools/rustdoc.rs\"\n",
    "[target.'cfg(all())']\nrunner = \"tools/run.rs\"\n",
    "[target.x86_64-unknown-linux-gnu]\nlinker = \"tools/link.rs\"\n",
    "[build]\nrustflags = \"-C linker=tools/link.rs\"\n",
    "[build]\nrustflags = [\"-C\", \"linker=tools/link.rs\"]\n",
    "[target.'cfg(unix)']\nrustflags = [\"-Clinker=tools/link.rs\"]\n",
    "[build]\nrustflags = [\"--codegen=linker=tools/link.rs\"]\n",
    "[build]\nrustflags = [\"--codegen\", \"linker=tools/link.rs\"]\n",
    "[target.aarch64-apple-darwin]\nrustflags = [\"-Dwarnings\", \"-C\", \"linker=tools/link.rs\"]\n",
];

const HARMLESS: [&str; 4] = [
    "[build]\njobs = 2\n",
    "[alias]\nb = \"build\"\n",
    "[target.x86_64-unknown-linux-gnu]\nrustflags = [\"-C\", \"opt-level=3\"]\n",
    "[build]\nrustflags = [\"-C\", \"link-dead-code\"]\n",
];

#[test]
fn a_cargo_config_that_names_a_program_seals_the_check() -> TestResult {
    for config in PROGRAMS {
        for file in [".cargo/config.toml", ".cargo/config"] {
            let mut view = world()?;
            put(&mut view, file, config)?;
            assert!(runs_every(&view, &QUIET)?, "{file}: {config}");
        }
    }
    for config in HARMLESS {
        let mut view = world()?;
        put(&mut view, ".cargo/config.toml", config)?;
        assert!(skips_every(&view, &QUIET)?, "{config}");
    }
    let mut elsewhere = world()?;
    put(&mut elsewhere, "tools/.cargo/config.toml", PROGRAMS[4])?;
    assert!(skips_every(&elsewhere, &QUIET)?);
    Ok(())
}

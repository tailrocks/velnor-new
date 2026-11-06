// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use crate::gc1_scene::{Scene, fail};
use crate::gc1_seal_support::Gh;

pub(crate) struct SupervisorClock {
    directory: PathBuf,
    real_python: PathBuf,
}

impl SupervisorClock {
    pub(crate) fn install(scene: &Scene, gh: &Gh) -> Result<Self, Box<dyn Error>> {
        let real_python = PathBuf::from(scene.restricted_path(&["python3"])?).join("python3");
        scene.install_shim(
            "python3",
            r#"#!/bin/sh
exec "$FAKE_REAL_PYTHON3" -I -c '
import glob, os, sys, time
elapsed = [0.0]
real_monotonic = time.monotonic
base_monotonic = real_monotonic()
directory = os.environ["FAKE_GH_DIR"]
def monotonic():
    for block in glob.glob(os.path.join(directory, "clock-block-control-*")):
        label = os.path.basename(block).removeprefix("clock-block-control-")
        ack = os.path.join(directory, "clock-block-ack-control-" + label)
        release = os.path.join(directory, "clock-block-release-control-" + label)
        if not os.path.exists(ack):
            with open(ack, "x", encoding="utf-8"):
                pass
            while not os.path.exists(release):
                time.sleep(0.01)
    for marker in glob.glob(os.path.join(directory, "clock-marker-*")):
        if marker.endswith(".next"):
            continue
        ack = marker.replace("clock-marker-", "clock-ack-", 1)
        if not os.path.exists(ack):
            with open(marker, encoding="utf-8") as request:
                elapsed[0] += float(request.read().strip())
            with open(ack, "x", encoding="utf-8"):
                pass
    base = base_monotonic if os.environ.get("FAKE_SUPERVISOR_CLOCK_FREEZE") == "1" else real_monotonic()
    return base + elapsed[0]
time.monotonic = monotonic
exec(compile(sys.argv[1], "<selector>", "exec"), {"__name__": "__main__"})
' "$3"
"#,
        )?;
        Ok(Self {
            directory: PathBuf::from(gh.dir()),
            real_python,
        })
    }

    pub(crate) fn real_python(&self) -> &Path {
        &self.real_python
    }

    pub(crate) fn advance(&self, label: &str, seconds: u64) -> Result<(), Box<dyn Error>> {
        if seconds == 0
            || !label
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '-')
        {
            return Err(fail("invalid supervisor clock advance"));
        }
        let marker = self.directory.join(format!("clock-marker-control-{label}"));
        let pending = marker.with_extension("next");
        std::fs::write(&pending, format!("{seconds}\n"))?;
        std::fs::rename(pending, marker)?;
        Ok(())
    }

    pub(crate) fn wait_for_advance(
        &self,
        label: &str,
        limit: Duration,
    ) -> Result<(), Box<dyn Error>> {
        let ack = self.directory.join(format!("clock-ack-control-{label}"));
        let deadline = Instant::now() + limit;
        while Instant::now() < deadline {
            if ack.exists() {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(10));
        }
        Err(fail(&format!(
            "supervisor did not acknowledge clock advance {label}"
        )))
    }

    pub(crate) fn block(&self, label: &str) -> Result<(), Box<dyn Error>> {
        std::fs::write(
            self.directory.join(format!("clock-block-control-{label}")),
            "block\n",
        )?;
        Ok(())
    }

    pub(crate) fn wait_for_block(
        &self,
        label: &str,
        limit: Duration,
    ) -> Result<(), Box<dyn Error>> {
        let ack = self
            .directory
            .join(format!("clock-block-ack-control-{label}"));
        let deadline = Instant::now() + limit;
        while Instant::now() < deadline {
            if ack.exists() {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(10));
        }
        Err(fail(&format!("supervisor did not block at clock {label}")))
    }

    pub(crate) fn release_block(&self, label: &str) -> Result<(), Box<dyn Error>> {
        std::fs::write(
            self.directory
                .join(format!("clock-block-release-control-{label}")),
            "release\n",
        )?;
        Ok(())
    }
}

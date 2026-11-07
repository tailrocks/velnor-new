// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use crate::gc1_scene::{Scene, fail};
use crate::gc1_seal_support::Gh;

const SUPERVISOR_CLOCK_SHIM: &str = r#"#!/bin/sh
exec "$FAKE_REAL_PYTHON3" -I -c '
import glob, os, signal, subprocess, sys, time
real_popen = subprocess.Popen
real_killpg = os.killpg
real_monotonic = time.monotonic
class BarrierPopen(real_popen):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        env = kwargs.get("env", {})
        inventory = os.environ.get("FAKE_SUPERVISOR_LAUNCH_INVENTORY")
        if inventory:
            try:
                with open(inventory, "a", encoding="utf-8") as record:
                    record.write(str(self.pid) + " " + env.get("VELNOR_STATE_SELECT_TAG", "") + "\n")
                    record.flush()
            except BaseException:
                group_gone = False
                child_reaped = False
                end = real_monotonic() + 5
                while real_monotonic() < end and not (group_gone and child_reaped):
                    try:
                        real_killpg(self.pid, signal.SIGKILL)
                    except ProcessLookupError:
                        group_gone = True
                    except OSError:
                        pass
                    child_reaped = self.poll() is not None
                    if not (group_gone and child_reaped):
                        time.sleep(0.01)
                if not child_reaped:
                    try:
                        self.kill()
                    except OSError:
                        pass
                    end = real_monotonic() + 1
                    while real_monotonic() < end and not child_reaped:
                        child_reaped = self.poll() is not None
                        if not child_reaped:
                            time.sleep(0.01)
                raise
        gate = os.environ.get("FAKE_SUPERVISOR_LAUNCH_GATE")
        if gate and env.get("VELNOR_STATE_SELECT_TAG") == os.environ.get("FAKE_SUPERVISOR_LAUNCH_TAG", "u1"):
            with open(gate + ".started", "w", encoding="utf-8") as record:
                record.write(str(self.pid))
            hold = os.environ.get("FAKE_SUPERVISOR_LAUNCH_HOLD")
            while hold and not os.path.exists(hold):
                time.sleep(0.01)
            while not os.path.exists(gate):
                time.sleep(0.01)
subprocess.Popen = BarrierPopen
def killpg(group, signum):
    launch_gate = os.environ.get("FAKE_SUPERVISOR_LAUNCH_GATE")
    launch_key = os.environ.get("FAKE_SUPERVISOR_LAUNCH_KEY")
    if launch_gate and launch_key and signum == signal.SIGTERM:
        started = launch_gate + ".started"
        if os.path.exists(started) and open(started, encoding="utf-8").read() == str(group):
            subjects = os.path.join(os.environ["FAKE_GH_DIR"], "subjects")
            end = real_monotonic() + 1
            while real_monotonic() < end:
                if os.path.exists(subjects) and launch_key in open(subjects, encoding="utf-8").read():
                    break
                time.sleep(0.01)
    log = os.environ.get("FAKE_SUPERVISOR_GROUP_LOG")
    if log and signum == signal.SIGKILL:
        with open(log, "a", encoding="utf-8") as record:
            record.write(str(group) + "\n")
    gate = os.environ.get("FAKE_SUPERVISOR_CLEANUP_GATE")
    state = os.environ.get("VELNOR_STATE_SELECT_DIR")
    if gate and signum == signal.SIGKILL and os.path.exists(os.path.join(state, "status-u0")):
        with open(gate + ".blocked", "w", encoding="utf-8"):
            pass
        while not os.path.exists(gate):
            time.sleep(0.01)
    return real_killpg(group, signum)
os.killpg = killpg
elapsed = [0.0]
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
"#;

pub(crate) struct SupervisorClock {
    directory: PathBuf,
    real_python: PathBuf,
}

pub(crate) struct ReleaseMarker {
    path: PathBuf,
    armed: bool,
}

impl ReleaseMarker {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }

    pub(crate) fn release(&mut self) -> Result<(), Box<dyn Error>> {
        if self.armed {
            std::fs::write(&self.path, "release\n")?;
            self.armed = false;
        }
        Ok(())
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for ReleaseMarker {
    fn drop(&mut self) {
        if self.armed {
            drop(std::fs::write(&self.path, "release\n"));
        }
    }
}

pub(crate) struct ClockBlock {
    release: ReleaseMarker,
}

impl ClockBlock {
    pub(crate) fn release(&mut self) -> Result<(), Box<dyn Error>> {
        self.release.release()
    }
}

impl SupervisorClock {
    pub(crate) fn install(scene: &Scene, gh: &Gh) -> Result<Self, Box<dyn Error>> {
        let real_python = PathBuf::from(scene.restricted_path(&["python3"])?).join("python3");
        scene.install_shim("python3", SUPERVISOR_CLOCK_SHIM)?;
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
        let ack = self.directory.join(format!("clock-ack-control-{label}"));
        if marker.exists() || ack.exists() {
            return Err(fail(&format!(
                "duplicate supervisor clock advance label {label}; use a unique label"
            )));
        }
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

    pub(crate) fn block_guarded(&self, label: &str) -> Result<ClockBlock, Box<dyn Error>> {
        let release = ReleaseMarker::new(
            self.directory
                .join(format!("clock-block-release-control-{label}")),
        );
        std::fs::write(
            self.directory.join(format!("clock-block-control-{label}")),
            "block\n",
        )?;
        Ok(ClockBlock { release })
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
}

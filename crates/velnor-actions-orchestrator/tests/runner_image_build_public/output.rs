// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
use std::{
    os::unix::fs::{MetadataExt, PermissionsExt},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

pub(super) fn mutate_readonly_context(path: &Path) -> Result<(), Box<dyn Error>> {
    let original = fs::read(path)?;
    let mut changed = original.clone();
    *changed.first_mut().ok_or("empty context")? ^= 1;
    let mode = fs::metadata(path)?.permissions().mode() & 0o7777;
    fs::set_permissions(path, fs::Permissions::from_mode(mode | 0o200))?;
    let write = fs::write(path, &changed);
    let restore = fs::set_permissions(path, fs::Permissions::from_mode(mode));
    write?;
    restore?;
    if fs::read(path)? != changed {
        return Err("context mutation did not persist".into());
    }
    Ok(())
}

struct CountedBuildx {
    inner: ExternalBuildx,
    calls: AtomicUsize,
}

impl RunnerImageBuildExecution for CountedBuildx {
    fn run_build(
        &self,
        command: &IsolatedCommand,
        deadline: CheckDeadline,
    ) -> Result<ProcessOutput, MiseError> {
        self.run(command, deadline)
    }

    fn run(
        &self,
        command: &IsolatedCommand,
        deadline: CheckDeadline,
    ) -> Result<ProcessOutput, MiseError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.inner.run(command, deadline)
    }
}

#[test]
fn preserves_every_existing_destination_and_rejects_unprivate_paths() -> Result<(), Box<dyn Error>>
{
    frozen_clock(|| {
        let fixture = Fixture::new()?;
        let parent = fixture.output.parent().ok_or("output parent")?;
        for kind in ["file", "empty", "nonempty", "symlink", "dangling"] {
            let output = parent.join(format!("existing-{kind}"));
            match kind {
                "file" => fs::write(&output, b"canary")?,
                "empty" => fs::create_dir(&output)?,
                "nonempty" => {
                    fs::create_dir(&output)?;
                    fs::write(output.join("canary"), b"canary")?;
                }
                "symlink" => std::os::unix::fs::symlink(&fixture.root, &output)?,
                _ => std::os::unix::fs::symlink(parent.join("missing"), &output)?,
            }
            let before = fs::symlink_metadata(&output)?;
            let mut request = fixture.request()?;
            request.output_dir = output.clone();
            let execution = CountedBuildx {
                inner: fixture.execution(),
                calls: AtomicUsize::new(0),
            };
            assert!(
                build_runner_images_with_execution(&request, &execution).is_err(),
                "accepted existing {kind}"
            );
            assert_eq!(
                execution.calls.load(Ordering::Relaxed),
                0,
                "existing {kind} invoked Buildx"
            );
            let after = fs::symlink_metadata(&output)?;
            assert_eq!((before.dev(), before.ino()), (after.dev(), after.ino()));
            match kind {
                "file" => assert_eq!(fs::read(&output)?, b"canary"),
                "nonempty" => assert_eq!(fs::read(output.join("canary"))?, b"canary"),
                "empty" => assert_eq!(fs::read_dir(&output)?.count(), 0),
                _ => {}
            }
        }
        let unprivate = parent.join("unprivate");
        fs::create_dir(&unprivate)?;
        fs::set_permissions(&unprivate, fs::Permissions::from_mode(0o755))?;
        let mut request = fixture.request()?;
        request.output_dir = unprivate.join("family");
        assert!(build_runner_images_with_execution(&request, &fixture.execution()).is_err());
        assert!(!request.output_dir.exists());
        request.output_dir = fixture.root.join("family");
        assert!(build_runner_images_with_execution(&request, &fixture.execution()).is_err());
        assert!(!request.output_dir.exists());
        assert!(!fs::read_dir(parent)?.any(|entry| entry.is_ok_and(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".velnor-runner-author-")
        })));
        Ok(())
    })
}

struct RacingBuildx {
    inner: ExternalBuildx,
    from: PathBuf,
    to: PathBuf,
    moved: AtomicBool,
}

impl RunnerImageBuildExecution for RacingBuildx {
    fn run_build(
        &self,
        command: &IsolatedCommand,
        deadline: CheckDeadline,
    ) -> Result<ProcessOutput, MiseError> {
        self.run(command, deadline)
    }

    fn run(
        &self,
        command: &IsolatedCommand,
        deadline: CheckDeadline,
    ) -> Result<ProcessOutput, MiseError> {
        if !self.moved.swap(true, Ordering::Relaxed) {
            fs::rename(&self.from, &self.to).map_err(|error| MiseError::SpawnFailed {
                program: "fixture filesystem".into(),
                message: error.to_string(),
            })?;
        }
        self.inner.run(command, deadline)
    }
}

#[test]
fn destination_created_during_build_is_never_replaced() -> Result<(), Box<dyn Error>> {
    frozen_clock(|| {
        let fixture = Fixture::new()?;
        let parent = fixture.output.parent().ok_or("output parent")?;
        let foreign = parent.join("racing-destination");
        fs::create_dir(&foreign)?;
        let before = fs::metadata(&foreign)?;
        let canary = parent.join("foreign-canary");
        fs::write(&canary, b"foreign canary")?;
        let execution = RacingBuildx {
            inner: fixture.execution(),
            from: foreign,
            to: fixture.output.clone(),
            moved: AtomicBool::new(false),
        };
        let error = match build_runner_images_with_execution(&fixture.request()?, &execution) {
            Ok(()) => return Err("raced destination was replaced".into()),
            Err(error) => error,
        };
        assert!(
            error.to_string().contains("runner_builder_publish"),
            "{error}"
        );
        assert!(execution.moved.load(Ordering::Relaxed));
        let after = fs::metadata(&fixture.output)?;
        assert_eq!((before.dev(), before.ino()), (after.dev(), after.ino()));
        assert_eq!(fs::read_dir(&fixture.output)?.count(), 0);
        assert_eq!(fs::read(canary)?, b"foreign canary");
        Ok(())
    })
}

#[derive(Default)]
struct PublicationGate {
    state: Mutex<PublicationGateState>,
    changed: Condvar,
}

#[derive(Default)]
struct PublicationGateState {
    second_build_complete: bool,
    release_second: bool,
}

impl PublicationGate {
    fn wait_for_second_build(&self, timeout: Duration) -> bool {
        let Ok(state) = self.state.lock() else {
            return false;
        };
        self.changed
            .wait_timeout_while(state, timeout, |state| !state.second_build_complete)
            .is_ok_and(|(state, _)| state.second_build_complete)
    }

    fn hold_second_until_release(&self) -> Result<(), MiseError> {
        let mut state = self.state.lock().map_err(|_| Self::gate_error())?;
        state.second_build_complete = true;
        self.changed.notify_all();
        let (state, _) = self
            .changed
            .wait_timeout_while(state, Duration::from_secs(20), |state| {
                !state.release_second
            })
            .map_err(|_| Self::gate_error())?;
        if state.release_second {
            Ok(())
        } else {
            Err(Self::gate_error())
        }
    }

    fn release(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.release_second = true;
            self.changed.notify_all();
        }
    }

    fn gate_error() -> MiseError {
        MiseError::SpawnFailed {
            program: "fixture publication gate".into(),
            message: "timed out or poisoned".into(),
        }
    }
}

struct ReleaseSecond(Arc<PublicationGate>);

impl Drop for ReleaseSecond {
    fn drop(&mut self) {
        self.0.release();
    }
}

struct GatedBuildx {
    inner: ExternalBuildx,
    gate: Arc<PublicationGate>,
}

impl RunnerImageBuildExecution for GatedBuildx {
    fn run_build(
        &self,
        command: &IsolatedCommand,
        deadline: CheckDeadline,
    ) -> Result<ProcessOutput, MiseError> {
        self.run(command, deadline)
    }

    fn run(
        &self,
        command: &IsolatedCommand,
        deadline: CheckDeadline,
    ) -> Result<ProcessOutput, MiseError> {
        let output = self.inner.run(command, deadline)?;
        if command
            .argv()
            .iter()
            .any(|argument| argument == "velnor-resource-probe:linux-amd64")
        {
            self.gate.hold_second_until_release()?;
        }
        Ok(output)
    }
}

#[test]
fn concurrent_builds_publish_exactly_one_complete_family() -> Result<(), Box<dyn Error>> {
    frozen_clock(|| {
        let first_fixture = Fixture::new()?;
        let second_fixture = Fixture::new()?;
        assert_ne!(first_fixture.contexts, second_fixture.contexts);
        let first_request = first_fixture.request()?;
        let mut second_request = second_fixture.request()?;
        second_request.output_dir = first_fixture.output.clone();
        let gate = Arc::new(PublicationGate::default());
        std::thread::scope(|scope| -> Result<(), Box<dyn Error>> {
            let _release = ReleaseSecond(Arc::clone(&gate));
            let first = scope.spawn(|| {
                frozen_clock(|| {
                    build_runner_images_with_execution(&first_request, &first_fixture.execution())
                })
            });
            let second_gate = Arc::clone(&gate);
            let second = scope.spawn(move || {
                frozen_clock(|| {
                    let execution = GatedBuildx {
                        inner: second_fixture.execution(),
                        gate: second_gate,
                    };
                    build_runner_images_with_execution(&second_request, &execution)
                })
            });
            let second_build_complete = gate.wait_for_second_build(Duration::from_secs(15));
            let first = first.join().map_err(|_| "first thread panicked")?;
            gate.release();
            let second = second.join().map_err(|_| "second thread panicked")?;
            assert!(
                second_build_complete,
                "second Buildx did not finish valid external work"
            );
            assert!(first.is_ok(), "first publication failed: {first:?}");
            let second_error = match second {
                Ok(()) => return Err("second build replaced a complete published family".into()),
                Err(error) => error,
            };
            assert!(
                second_error.to_string().contains("runner_builder_publish"),
                "{second_error}"
            );
            Ok(())
        })?;
        assert_eq!(fs::read_dir(&first_fixture.output)?.count(), 10);
        for name in [
            "velnor-runner-linux-amd64.tar",
            "velnor-dind-linux-amd64.tar",
            "velnor-resource-probe-linux-amd64.tar",
            "runner-image-provenance.json",
            "dind-image-provenance.json",
            "resource-probe-image-provenance.json",
            "resource_probe",
            "storage_init",
            "job_start_hook",
            "job_started_hook",
        ] {
            assert!(first_fixture.output.join(name).is_file(), "missing {name}");
        }
        assert!(
            !fs::read_dir(first_fixture.output.parent().ok_or("output parent")?)?.any(|entry| {
                entry.is_ok_and(|entry| {
                    entry
                        .file_name()
                        .to_string_lossy()
                        .starts_with(".velnor-runner-author-")
                })
            })
        );
        Ok(())
    })
}

#[test]
fn changing_held_context_during_build_prevents_family_output() -> Result<(), Box<dyn Error>> {
    frozen_clock(|| {
        let fixture = Fixture::new()?;
        let mut execution = fixture.execution();
        let mutated = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        execution.mutate_context = Some(std::sync::Arc::clone(&mutated));
        let result = build_runner_images_with_execution(&fixture.request()?, &execution);
        assert!(mutated.load(std::sync::atomic::Ordering::Acquire));
        let error = match result {
            Ok(()) => return Err("changed held context was accepted".into()),
            Err(error) => error,
        };
        assert!(
            error.to_string().contains("runner_context_changed"),
            "{error}"
        );
        assert!(!fixture.output.exists());
        assert_eq!(
            fs::read(fixture.contexts.join("resource_probe"))?,
            b"fixture resource_probe"
        );
        Ok(())
    })
}

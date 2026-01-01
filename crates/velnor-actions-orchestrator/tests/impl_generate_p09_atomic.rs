//! Root-visibility diagnostics for atomic `.github` replacement.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use super::impl_common::{TestResult, config_with_branch, make_repo};
use velnor_actions_orchestrator::{GenerateOptions, generate, prepare};

const WRITER_COMPLETE: usize = 17;

#[derive(Default)]
struct ErrorCounts {
    not_found: AtomicUsize,
    other: AtomicUsize,
}

impl ErrorCounts {
    fn record(&self, error: &io::Error) {
        match error.kind() {
            io::ErrorKind::NotFound => {
                self.not_found.fetch_add(1, Ordering::Relaxed);
            }
            _ => {
                self.other.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    fn snapshot(&self) -> (usize, usize) {
        (
            self.not_found.load(Ordering::Relaxed),
            self.other.load(Ordering::Relaxed),
        )
    }
}

struct FirstMetadataError {
    observer: usize,
    iteration: u64,
    wall_unix_ns: Option<u128>,
    elapsed_ns: u128,
    writer_phase: String,
    path: PathBuf,
    kind: io::ErrorKind,
    raw_errno: Option<i32>,
    display: String,
    parent_probe: String,
    target_probe: String,
}

impl FirstMetadataError {
    fn capture(
        observer: usize,
        iteration: u64,
        target: &Path,
        parent: &Path,
        error: &io::Error,
        phase: usize,
        started: &Instant,
    ) -> Self {
        let wall_unix_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .map(|duration| duration.as_nanos());
        let elapsed_ns = started.elapsed().as_nanos();
        let writer_phase = writer_phase_label(phase);
        let path = target.to_path_buf();
        let kind = error.kind();
        let raw_errno = error.raw_os_error();
        let display = error.to_string();
        let parent_probe = metadata_probe(parent);
        let target_probe = metadata_probe(target);
        Self {
            observer,
            iteration,
            wall_unix_ns,
            elapsed_ns,
            writer_phase,
            path,
            kind,
            raw_errno,
            display,
            parent_probe,
            target_probe,
        }
    }

    fn emit(self) {
        eprintln!(
            "FIRST_METADATA_ERROR wall_unix_ns={:?} elapsed_ns={} observer={} iteration={} writer_phase={} path={} kind={:?} raw_errno={:?} display={:?} POST_ERROR_PARENT_PROBE[{}] POST_ERROR_TARGET_PROBE[{}]",
            self.wall_unix_ns,
            self.elapsed_ns,
            self.observer,
            self.iteration,
            self.writer_phase,
            self.path.display(),
            self.kind,
            self.raw_errno,
            self.display,
            self.parent_probe,
            self.target_probe
        );
    }
}

fn writer_phase_label(phase: usize) -> String {
    match phase {
        0 => "writer_not_started".to_owned(),
        WRITER_COMPLETE => "writer_complete".to_owned(),
        value if value % 2 == 1 => format!("rewrite_{}_in_progress", value.div_ceil(2)),
        value => format!("rewrite_{}_completed", value / 2),
    }
}

fn metadata_probe(path: &Path) -> String {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => format!(
            "path={},status=ok,is_dir={},is_symlink={}",
            path.display(),
            metadata.is_dir(),
            metadata.file_type().is_symlink()
        ),
        Err(error) => format!(
            "path={},status=err,kind={:?},raw_errno={:?},display={:?}",
            path.display(),
            error.kind(),
            error.raw_os_error(),
            error
        ),
    }
}

fn observe_until_done(
    observer: usize,
    parent: &Path,
    target: &Path,
    done: &AtomicUsize,
    counts: &ErrorCounts,
    writer_phase: &AtomicUsize,
    started: &Instant,
) {
    let mut iteration = 0_u64;
    let mut first_error = None;
    while done.load(Ordering::Relaxed) == 0 {
        iteration += 1;
        if let Err(error) = target.symlink_metadata() {
            counts.record(&error);
            if first_error.is_none() {
                first_error = Some(FirstMetadataError::capture(
                    observer,
                    iteration,
                    target,
                    parent,
                    &error,
                    writer_phase.load(Ordering::Relaxed),
                    started,
                ));
            }
        }
    }
    if let Some(first_error) = first_error {
        first_error.emit();
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn atomic_commit_never_exposes_missing_tree() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    let prep = prepare(root)?;
    let opts = GenerateOptions { output_dir: None };
    generate(&prep, &opts)?;
    let live = root.join(".github");
    let counts = ErrorCounts::default();
    let done = AtomicUsize::new(0);
    let writer_phase = AtomicUsize::new(0);
    let started = Instant::now();
    std::thread::scope(|scope| {
        scope.spawn(|| {
            for rewrite in 1..=8 {
                writer_phase.store(rewrite * 2 - 1, Ordering::Relaxed);
                generate(&prep, &opts).expect("rewrite commits");
                writer_phase.store(rewrite * 2, Ordering::Relaxed);
            }
            writer_phase.store(WRITER_COMPLETE, Ordering::Relaxed);
            done.store(1, Ordering::Relaxed);
        });
        for observer in 0..4 {
            let parent = root.to_path_buf();
            let target = live.clone();
            let done_ref = &done;
            let counts_ref = &counts;
            let phase_ref = &writer_phase;
            let started_ref = &started;
            scope.spawn(move || {
                observe_until_done(
                    observer,
                    &parent,
                    &target,
                    done_ref,
                    counts_ref,
                    phase_ref,
                    started_ref,
                );
            });
        }
    });
    assert!(live.is_dir(), "final tree live");
    let (not_found, other) = counts.snapshot();
    assert!(
        not_found == 0 && other == 0,
        "the root must never be absent during exchange; NotFound errors={not_found}, other metadata errors={other}"
    );
    Ok(())
}

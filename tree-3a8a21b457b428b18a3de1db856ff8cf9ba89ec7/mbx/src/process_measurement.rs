//! Scoped, single-use invocation and real child-process measurements.
//! No adapter preparation, cache lookup, or telemetry delivery enters child wall.

use crate::measurement_reliability::{Delivery, EventKind};
use mbx_cache_core::{
    AdapterKind, CacheOutcome, InvocationKind, MeasurementEvent, OutputObservation,
    ProcessMeasurement, ProcessOutcome, ProcessPurpose, UnitIdentity,
};
use std::io;
use std::process::{Child, ChildStderr, ChildStdout, Command, ExitStatus, Output, Stdio};
use std::time::Instant;

type SessionSink = fn(MeasurementEvent);

mod delivery;
use delivery::{deliver, enroll, ignore_sink};

#[derive(Default)]
struct ProcessProgress {
    work_attempted: bool,
}

/// One terminal cache disposition, independent of how many children it starts.
/// Dropping without a disposition records an explicit unknown invocation.
pub(crate) struct Invocation<S: FnMut(MeasurementEvent) = SessionSink> {
    adapter: AdapterKind,
    kind: InvocationKind,
    unit: Option<UnitIdentity>,
    sink: S,
    finished: bool,
    outcome: CacheOutcome,
    progress: ProcessProgress,
    native: bool,
    delivery: Option<Delivery>,
}

impl Invocation<SessionSink> {
    pub(crate) fn new(
        adapter: AdapterKind,
        kind: InvocationKind,
        unit: Option<UnitIdentity>,
    ) -> Self {
        let mut invocation = Self::initialize(adapter, kind, unit, ignore_sink);
        invocation.native = true;
        invocation.delivery = enroll(adapter, EventKind::Invocation);
        invocation
    }
}

impl<S: FnMut(MeasurementEvent)> Invocation<S> {
    #[cfg(test)]
    pub(crate) fn with_sink(
        adapter: AdapterKind,
        kind: InvocationKind,
        unit: Option<UnitIdentity>,
        sink: S,
    ) -> Self {
        Self::initialize(adapter, kind, unit, sink)
    }

    fn initialize(
        adapter: AdapterKind,
        kind: InvocationKind,
        unit: Option<UnitIdentity>,
        sink: S,
    ) -> Self {
        Self {
            adapter,
            kind,
            unit,
            sink,
            finished: false,
            outcome: CacheOutcome::Unknown,
            progress: ProcessProgress::default(),
            native: false,
            delivery: None,
        }
    }

    pub(crate) fn identify(&mut self, unit: Option<UnitIdentity>) {
        self.unit = unit;
    }

    /// Record directly observed output without minting another invocation.
    pub(crate) fn record_output(&mut self, mut observation: OutputObservation) {
        observation.cache_outcome = self.outcome;
        let event = MeasurementEvent::Output {
            adapter: self.adapter,
            unit: self.unit.clone(),
            observation,
        };
        if self.native {
            let delivery = enroll(self.adapter, EventKind::Output);
            deliver(event, delivery);
        } else {
            (self.sink)(event);
        }
    }

    /// Update the terminal disposition while the outer wrapper retains the
    /// single invocation token. Its eventual drop emits that disposition once.
    pub(crate) fn set_outcome(&mut self, outcome: CacheOutcome) {
        self.outcome = outcome;
    }

    pub(crate) fn outcome(&self) -> CacheOutcome {
        self.outcome
    }

    #[cfg(test)]
    pub(crate) fn finish_current(mut self) {
        self.record(self.outcome);
    }

    /// Flush before a platform exit primitive that does not run destructors.
    #[cfg(any(windows, test))]
    pub(crate) fn finish_now(&mut self) {
        if !self.finished {
            self.record(self.outcome);
        }
    }

    /// Once a workload spawn was attempted, fallback cannot safely retry the
    /// invocation even when the operating system failed to start that child.
    pub(crate) fn has_work_attempted(&self) -> bool {
        self.progress.work_attempted
    }

    /// This token does not count an attempt until `spawn` is called.
    pub(crate) fn process(&mut self, purpose: ProcessPurpose) -> ProcessToken<'_, S> {
        ProcessToken {
            adapter: self.adapter,
            purpose,
            unit: self.unit.clone(),
            sink: &mut self.sink,
            progress: Some(&mut self.progress),
            native: self.native,
            delivery: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn finish(mut self, outcome: CacheOutcome) {
        self.record(outcome);
    }

    fn record(&mut self, cache_outcome: CacheOutcome) {
        if self.finished {
            return;
        }
        self.finished = true;
        let event = MeasurementEvent::Invocation {
            adapter: self.adapter,
            invocation_kind: self.kind,
            cache_outcome,
            unit: self.unit.take(),
        };
        if self.native {
            deliver(event, self.delivery.take());
        } else {
            (self.sink)(event);
        }
    }
}

impl<S: FnMut(MeasurementEvent)> Drop for Invocation<S> {
    fn drop(&mut self) {
        if !self.finished {
            self.record(self.outcome);
        }
    }
}

/// A single-use permission to attempt one operating-system child spawn.
pub(crate) struct ProcessToken<'a, S: FnMut(MeasurementEvent)> {
    adapter: AdapterKind,
    purpose: ProcessPurpose,
    unit: Option<UnitIdentity>,
    sink: &'a mut S,
    progress: Option<&'a mut ProcessProgress>,
    native: bool,
    delivery: Option<Delivery>,
}

impl<'a, S: FnMut(MeasurementEvent)> ProcessToken<'a, S> {
    pub(crate) fn spawn(mut self, command: &mut Command) -> io::Result<MeasuredChild<'a, S>> {
        if self.native {
            self.delivery = enroll(self.adapter, EventKind::Process);
        }
        if self.purpose != ProcessPurpose::Probe
            && let Some(progress) = &mut self.progress
        {
            progress.work_attempted = true;
        }
        let started = Instant::now();
        match command.spawn() {
            Ok(child) => Ok(MeasuredChild {
                child: Some(child),
                token: Some(self),
                started,
            }),
            Err(error) => {
                self.record(ProcessOutcome::SpawnFailed, false, None);
                Err(error)
            }
        }
    }

    pub(crate) fn status(self, command: &mut Command) -> io::Result<ExitStatus> {
        self.spawn(command)?.wait()
    }

    /// Capture both output streams with null input, matching the compiler
    /// supervision path. Callers requiring streaming use `spawn` instead.
    pub(crate) fn output(self, command: &mut Command) -> io::Result<Output> {
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        self.spawn(command)?.wait_with_output()
    }

    fn record(self, outcome: ProcessOutcome, started: bool, wall_ns: Option<u64>) {
        let event = MeasurementEvent::Process {
            adapter: self.adapter,
            purpose: self.purpose,
            outcome,
            measurement: ProcessMeasurement {
                attempts: 1,
                started: u64::from(started),
                observed_wall_ns: wall_ns.unwrap_or(0),
                wall_observations: u64::from(wall_ns.is_some()),
            },
            unit: self.unit,
        };
        if self.native {
            deliver(event, self.delivery);
        } else {
            (self.sink)(event);
        }
    }
}

/// A successfully started child. Its terminal observer is consumed by wait.
/// Dropping it records missing terminal observation and no wall sample.
pub(crate) struct MeasuredChild<'a, S: FnMut(MeasurementEvent)> {
    child: Option<Child>,
    token: Option<ProcessToken<'a, S>>,
    started: Instant,
}

impl<S: FnMut(MeasurementEvent)> MeasuredChild<'_, S> {
    pub(crate) fn take_stdout(&mut self) -> Option<ChildStdout> {
        self.child.as_mut().and_then(|child| child.stdout.take())
    }

    pub(crate) fn take_stderr(&mut self) -> Option<ChildStderr> {
        self.child.as_mut().and_then(|child| child.stderr.take())
    }

    pub(crate) fn wait(mut self) -> io::Result<ExitStatus> {
        let result = match self.child.as_mut() {
            Some(child) => child.wait(),
            None => Err(io::Error::other("measured child was already consumed")),
        };
        self.finished(result.as_ref().ok().copied());
        result
    }

    pub(crate) fn wait_with_output(mut self) -> io::Result<Output> {
        let result = match self.child.take() {
            Some(child) => child.wait_with_output(),
            None => Err(io::Error::other("measured child was already consumed")),
        };
        self.finished(result.as_ref().ok().map(|output| output.status));
        result
    }

    fn finished(&mut self, status: Option<ExitStatus>) {
        // Capture the boundary before delivering telemetry to the cache agent.
        let wall = self
            .started
            .elapsed()
            .as_nanos()
            .try_into()
            .unwrap_or(u64::MAX);
        if let Some(token) = self.token.take() {
            match status {
                Some(status) => token.record(outcome(status), true, Some(wall)),
                None => token.record(ProcessOutcome::WaitFailed, true, None),
            }
        }
    }
}

impl<S: FnMut(MeasurementEvent)> Drop for MeasuredChild<'_, S> {
    fn drop(&mut self) {
        if let Some(token) = self.token.take() {
            token.record(ProcessOutcome::WaitFailed, true, None);
        }
    }
}

fn outcome(status: ExitStatus) -> ProcessOutcome {
    if status.success() {
        ProcessOutcome::Succeeded
    } else if status.code().is_some() {
        ProcessOutcome::Failed
    } else {
        ProcessOutcome::Terminated
    }
}

/// Identity queries inside immutable cache-planning callbacks are actual
/// processes belonging to the existing outer invocation. They add no second
/// wrapper invocation and claim no unavailable unit attribution.
pub(crate) fn probe_output(adapter: AdapterKind, command: &mut Command) -> io::Result<Output> {
    let mut sink: SessionSink = ignore_sink;
    ProcessToken {
        adapter,
        purpose: ProcessPurpose::Probe,
        unit: None,
        sink: &mut sink,
        progress: None,
        native: true,
        delivery: None,
    }
    .output(command)
}

#[cfg(test)]
#[path = "process_measurement_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "process_measurement_lifecycle_tests.rs"]
mod lifecycle_tests;

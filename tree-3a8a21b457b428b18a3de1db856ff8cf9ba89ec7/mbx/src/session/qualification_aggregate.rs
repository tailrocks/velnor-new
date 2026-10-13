//! Checked verification of terminal receipts against direct owning totals.
use super::QualificationReason;
use mbx_cache_core::{
    AdapterKind, AdapterMeasurement, CacheOutcome, InvocationKind, MeasurementEvent,
    ProcessMeasurement, ProcessOutcome, ProcessPurpose, UnitMeasurement,
};
use std::collections::BTreeMap;

pub(super) fn add(
    totals: &mut BTreeMap<AdapterKind, AdapterMeasurement>,
    event: &MeasurementEvent,
) -> Result<(), QualificationReason> {
    let (adapter, identity) = match event {
        MeasurementEvent::Invocation { adapter, unit, .. }
        | MeasurementEvent::Process { adapter, unit, .. }
        | MeasurementEvent::Output { adapter, unit, .. } => (*adapter, unit.clone()),
    };
    let total = totals.entry(adapter).or_default();
    apply(&mut total.invocations, &mut total.subprocesses, event)?;
    let index = total
        .units
        .iter()
        .position(|row| row.identity == identity)
        .unwrap_or_else(|| {
            total.units.push(UnitMeasurement {
                identity,
                ..Default::default()
            });
            total.units.len() - 1
        });
    let row = &mut total.units[index];
    if let MeasurementEvent::Output { observation, .. } = event {
        if observation.digest.validate().is_err()
            || !observation.path.is_absolute()
            || observation.aliases.iter().any(|path| !path.is_absolute())
        {
            return Err(QualificationReason::InvalidTerminalEvent);
        }
        row.outputs.push(observation.clone());
    }
    apply(&mut row.invocations, &mut row.subprocesses, event)
}

type Invocations = BTreeMap<InvocationKind, BTreeMap<CacheOutcome, u64>>;
type Processes = BTreeMap<ProcessPurpose, BTreeMap<ProcessOutcome, ProcessMeasurement>>;

fn apply(
    invocations: &mut Invocations,
    processes: &mut Processes,
    event: &MeasurementEvent,
) -> Result<(), QualificationReason> {
    match event {
        MeasurementEvent::Invocation {
            invocation_kind,
            cache_outcome,
            ..
        } => {
            let count = invocations
                .entry(*invocation_kind)
                .or_default()
                .entry(*cache_outcome)
                .or_default();
            *count = count
                .checked_add(1)
                .ok_or(QualificationReason::AggregateOverflow)?;
        }
        MeasurementEvent::Process {
            purpose,
            outcome,
            measurement,
            ..
        } => {
            validate_process(*outcome, measurement)?;
            let total = processes
                .entry(*purpose)
                .or_default()
                .entry(*outcome)
                .or_default();
            merge_process(total, measurement)?;
        }
        MeasurementEvent::Output { .. } => {}
    }
    Ok(())
}

pub(super) fn sum(left: u64, right: u64) -> Result<u64, QualificationReason> {
    left.checked_add(right)
        .ok_or(QualificationReason::AggregateOverflow)
}

fn validate_process(
    outcome: ProcessOutcome,
    measurement: &ProcessMeasurement,
) -> Result<(), QualificationReason> {
    let (started, observations) = match outcome {
        ProcessOutcome::Succeeded | ProcessOutcome::Failed | ProcessOutcome::Terminated => (1, 1),
        ProcessOutcome::SpawnFailed => (0, 0),
        ProcessOutcome::WaitFailed => (1, 0),
    };
    if measurement.attempts != 1
        || measurement.started != started
        || measurement.wall_observations != observations
        || (observations == 0 && measurement.observed_wall_ns != 0)
    {
        return Err(QualificationReason::InvalidTerminalEvent);
    }
    Ok(())
}

pub(super) fn merge_process(
    total: &mut ProcessMeasurement,
    measurement: &ProcessMeasurement,
) -> Result<(), QualificationReason> {
    total.attempts = sum(total.attempts, measurement.attempts)?;
    total.started = sum(total.started, measurement.started)?;
    total.observed_wall_ns = sum(total.observed_wall_ns, measurement.observed_wall_ns)?;
    total.wall_observations = sum(total.wall_observations, measurement.wall_observations)?;
    Ok(())
}

#[path = "qualification_units.rs"]
mod units;

#[cfg(test)]
#[path = "qualification_output_tests.rs"]
mod output_tests;

pub(super) fn check(
    observed: &BTreeMap<AdapterKind, AdapterMeasurement>,
    receipts: &BTreeMap<AdapterKind, AdapterMeasurement>,
) -> Result<(), QualificationReason> {
    if observed.len() != receipts.len() {
        return Err(QualificationReason::AggregateCountMismatch);
    }
    for (adapter, actual) in observed {
        let expected = receipts
            .get(adapter)
            .ok_or(QualificationReason::AggregateCountMismatch)?;
        if actual.invocations != expected.invocations
            || actual.subprocesses != expected.subprocesses
        {
            return Err(QualificationReason::AggregateCountMismatch);
        }
        // Arrival order governs the native attribution cap. Unordered receipts
        // cannot prove which rows were omitted; qualification keeps that unknown.
        if actual.omitted_unit_events == 0
            && actual.omitted_output_events == 0
            && !actual
                .units
                .iter()
                .any(|unit| unit.omitted_output_events != 0)
            && !units::agrees(&actual.units, &expected.units)?
        {
            return Err(QualificationReason::UnitAggregateMismatch);
        }
    }
    Ok(())
}

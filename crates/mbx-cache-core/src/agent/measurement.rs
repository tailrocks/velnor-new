//! Bounded unit attribution with uncapped adapter totals.
use crate::{
    AdapterKind, AdapterMeasurement, MeasurementEvent, PackageOrigin, ProcessMeasurement,
    ProcessOutcome, UnitIdentity, UnitMeasurement,
};
use eyre::{Result, bail};
use std::collections::BTreeMap;
use std::sync::Mutex;

const MAX_UNITS_PER_ADAPTER: usize = 4096;
const MAX_OUTPUTS_PER_ADAPTER: usize = 4096;
const MAX_OUTPUT_BYTES_PER_ADAPTER: usize = 8 * 1024 * 1024;
type Measurements = Mutex<BTreeMap<AdapterKind, AdapterMeasurement>>;

pub(super) fn record(destination: &Measurements, event: MeasurementEvent) -> Result<()> {
    let (adapter, identity) = match &event {
        MeasurementEvent::Invocation { adapter, unit, .. }
        | MeasurementEvent::Process { adapter, unit, .. }
        | MeasurementEvent::Output { adapter, unit, .. } => (*adapter, unit),
    };
    validate_identity(identity.as_ref())?;
    if let MeasurementEvent::Process {
        outcome,
        measurement,
        ..
    } = &event
    {
        validate_process(*outcome, measurement)?;
    }
    if let MeasurementEvent::Output { observation, .. } = &event {
        validate_output(observation)?;
    }
    let mut adapters = destination
        .lock()
        .map_err(|_| eyre::eyre!("measurement lock poisoned"))?;
    let totals = adapters.entry(adapter).or_default();
    apply(&mut totals.invocations, &mut totals.subprocesses, &event);
    let existing = totals
        .units
        .iter()
        .position(|unit| unit.identity == *identity);
    let index = if let Some(index) = existing {
        index
    } else if identity.is_none()
        || totals
            .units
            .iter()
            .filter(|unit| unit.identity.is_some())
            .count()
            < MAX_UNITS_PER_ADAPTER
    {
        totals.units.push(UnitMeasurement {
            identity: identity.clone(),
            ..Default::default()
        });
        totals.units.len() - 1
    } else {
        totals.omitted_unit_events = totals.omitted_unit_events.saturating_add(1);
        if matches!(event, MeasurementEvent::Output { .. }) {
            totals.omitted_output_events = totals.omitted_output_events.saturating_add(1);
        }
        return Ok(());
    };
    let unit = &mut totals.units[index];
    apply(&mut unit.invocations, &mut unit.subprocesses, &event);
    if let MeasurementEvent::Output { observation, .. } = event {
        record_output(totals, index, observation);
    }
    Ok(())
}

pub(super) fn snapshot(destination: &Measurements) -> BTreeMap<AdapterKind, AdapterMeasurement> {
    let mut result = destination
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone();
    for adapter in result.values_mut() {
        adapter
            .units
            .sort_by(|left, right| left.identity.cmp(&right.identity));
    }
    result
}

type Invocations = BTreeMap<crate::InvocationKind, BTreeMap<crate::CacheOutcome, u64>>;
type Processes = BTreeMap<crate::ProcessPurpose, BTreeMap<ProcessOutcome, ProcessMeasurement>>;
fn apply(invocations: &mut Invocations, processes: &mut Processes, event: &MeasurementEvent) {
    match event {
        MeasurementEvent::Output { .. } => {}
        MeasurementEvent::Invocation {
            invocation_kind,
            cache_outcome,
            ..
        } => {
            let total = invocations
                .entry(*invocation_kind)
                .or_default()
                .entry(*cache_outcome)
                .or_default();
            *total = total.saturating_add(1);
        }
        MeasurementEvent::Process {
            purpose,
            outcome,
            measurement,
            ..
        } => {
            let total = processes
                .entry(*purpose)
                .or_default()
                .entry(*outcome)
                .or_default();
            total.attempts = total.attempts.saturating_add(measurement.attempts);
            total.started = total.started.saturating_add(measurement.started);
            total.observed_wall_ns = total
                .observed_wall_ns
                .saturating_add(measurement.observed_wall_ns);
            total.wall_observations = total
                .wall_observations
                .saturating_add(measurement.wall_observations);
        }
    }
}

fn record_output(
    totals: &mut AdapterMeasurement,
    index: usize,
    observation: crate::OutputObservation,
) {
    let count = totals
        .units
        .iter()
        .map(|unit| unit.outputs.len())
        .sum::<usize>();
    let bytes = totals
        .units
        .iter()
        .flat_map(|unit| &unit.outputs)
        .map(output_bytes)
        .fold(0usize, usize::saturating_add);
    if count >= MAX_OUTPUTS_PER_ADAPTER
        || bytes.saturating_add(output_bytes(&observation)) > MAX_OUTPUT_BYTES_PER_ADAPTER
    {
        totals.omitted_output_events = totals.omitted_output_events.saturating_add(1);
        let unit = &mut totals.units[index];
        unit.omitted_output_events = unit.omitted_output_events.saturating_add(1);
    } else {
        totals.units[index].outputs.push(observation);
    }
}
fn output_bytes(observation: &crate::OutputObservation) -> usize {
    observation
        .aliases
        .iter()
        .map(|path| path.as_os_str().len())
        .fold(observation.path.as_os_str().len(), usize::saturating_add)
        .saturating_add(observation.digest.hash.len())
        .saturating_add(
            observation
                .file_identity
                .as_ref()
                .map_or(0, |identity| identity.path.as_os_str().len()),
        )
}
fn validate_output(observation: &crate::OutputObservation) -> Result<()> {
    super::measurement_attribution::validate_path(&observation.path)?;
    observation.digest.validate()?;
    if let Some(identity) = &observation.file_identity
        && (identity.path != observation.path || identity.len != observation.digest.size)
    {
        bail!("measurement output identity disagrees with observed path or digest size");
    }
    if !observation.aliases.is_empty()
        && observation
            .file_identity
            .as_ref()
            .and_then(|identity| identity.object.as_ref())
            .is_none()
    {
        bail!("measurement output aliases lack observed physical object identity");
    }
    if observation.aliases.len() > 32 {
        bail!("measurement output alias count exceeds limits");
    }
    let mut paths = std::collections::BTreeSet::from([&observation.path]);
    for alias in &observation.aliases {
        super::measurement_attribution::validate_path(alias)?;
        if !paths.insert(alias) {
            bail!("measurement output aliases are duplicated");
        }
    }
    Ok(())
}

fn validate_identity(identity: Option<&UnitIdentity>) -> Result<()> {
    let Some(identity) = identity else {
        return Ok(());
    };
    if identity
        .cargo_unit_id
        .as_ref()
        .is_some_and(|value| value.is_empty() || value.len() > 256)
        || identity
            .package_id
            .as_ref()
            .is_some_and(|value| value.is_empty() || value.len() > 4096)
        || (identity.package_id.is_none() && identity.origin != PackageOrigin::Unknown)
        || (identity.provenance == crate::UnitProvenance::CargoPackageContext
            && identity.source_origin != PackageOrigin::Unknown)
    {
        bail!("measurement unit identity exceeds limits or lacks package provenance");
    }
    for path in [&identity.manifest_path, &identity.source_path]
        .into_iter()
        .flatten()
    {
        super::measurement_attribution::validate_path(path)?;
    }
    Ok(())
}

fn validate_process(outcome: ProcessOutcome, measurement: &ProcessMeasurement) -> Result<()> {
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
        bail!("measurement process event is not one consistent terminal attempt");
    }
    Ok(())
}

#[cfg(test)]
#[path = "measurement_tests.rs"]
mod tests;

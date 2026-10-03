//! Numeric unit fidelity independent of unavailable package authority.
use super::{QualificationReason, merge_process, sum};
use mbx_cache_core::{PackageOrigin, UnitIdentity, UnitMeasurement};
use std::collections::BTreeMap;

pub(super) fn agrees(
    left: &[UnitMeasurement],
    right: &[UnitMeasurement],
) -> Result<bool, QualificationReason> {
    Ok(normalize(left)? == normalize(right)?)
}

fn normalize(
    rows: &[UnitMeasurement],
) -> Result<BTreeMap<Option<UnitIdentity>, UnitMeasurement>, QualificationReason> {
    let mut result = BTreeMap::<Option<UnitIdentity>, UnitMeasurement>::new();
    for row in rows {
        let identity = row.identity.clone().map(|mut identity| {
            // These fields are resolved afterward from independent owning Cargo
            // metadata. Their authority is a separate qualification prerequisite.
            identity.package_id = None;
            identity.origin = PackageOrigin::Unknown;
            identity.source_origin = PackageOrigin::Unknown;
            identity
        });
        let total = result
            .entry(identity.clone())
            .or_insert_with(|| UnitMeasurement {
                identity,
                ..Default::default()
            });
        for (kind, outcomes) in &row.invocations {
            for (outcome, count) in outcomes {
                let destination = total
                    .invocations
                    .entry(*kind)
                    .or_default()
                    .entry(*outcome)
                    .or_default();
                *destination = sum(*destination, *count)?;
            }
        }
        for (purpose, outcomes) in &row.subprocesses {
            for (outcome, measurement) in outcomes {
                let destination = total
                    .subprocesses
                    .entry(*purpose)
                    .or_default()
                    .entry(*outcome)
                    .or_default();
                merge_process(destination, measurement)?;
            }
        }
        total.outputs.extend(row.outputs.clone());
        total.outputs.sort();
        total.omitted_output_events = sum(total.omitted_output_events, row.omitted_output_events)?;
    }
    Ok(result)
}

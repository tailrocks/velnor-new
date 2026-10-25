use std::time::Instant;

use crate::launch::pressure::Sample;

use super::Observation;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FreshPressureSample {
    sample: Sample,
    observed_at: Instant,
}

impl FreshPressureSample {
    pub(crate) fn sample_if_recent_at(self, now: Instant) -> Option<Sample> {
        now.checked_duration_since(self.observed_at)
            .filter(|age| *age <= super::MAX_OBSERVATION_AGE)
            .map(|_| self.sample)
    }
}

impl Observation {
    pub(in crate::launch::resource_probe) fn pressure_sample_at(
        &self,
        now: Instant,
    ) -> Option<FreshPressureSample> {
        self.is_recent_at(now).then_some(FreshPressureSample {
            sample: Sample {
                load_milli: self.record.load_milli,
                cpus: self.cpus,
                memory_available_bytes: self.record.memory_available_bytes,
                docker_root_free_bytes: Some(self.record.docker_root_free_bytes),
            },
            observed_at: self.observed_at,
        })
    }
}

#[cfg(test)]
impl FreshPressureSample {
    pub(crate) const fn test_new(sample: Sample, observed_at: Instant) -> Self {
        Self {
            sample,
            observed_at,
        }
    }
}

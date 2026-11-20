// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    error::Error,
    thread,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Observation {
    Counted,
    NotYetVisible,
    Faulted,
}

pub(crate) fn poll_observation<T>(
    limit: Duration,
    interval: Duration,
    mut observe: impl FnMut() -> Result<T, Box<dyn Error>>,
    classify: impl Fn(&T) -> Observation,
) -> Result<T, Box<dyn Error>> {
    let deadline = Instant::now() + limit;
    loop {
        let value = observe()?;
        match classify(&value) {
            Observation::Counted | Observation::Faulted => return Ok(value),
            Observation::NotYetVisible if Instant::now() >= deadline => return Ok(value),
            Observation::NotYetVisible => thread::sleep(interval),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIMIT: Duration = Duration::from_secs(5);
    const INTERVAL: Duration = Duration::from_millis(1);

    fn run(
        sequence: &[(u32, Observation)],
        limit: Duration,
    ) -> Result<(u32, usize), Box<dyn Error>> {
        let mut calls = 0;
        let value = poll_observation(
            limit,
            INTERVAL,
            || {
                let index = calls.min(sequence.len() - 1);
                calls += 1;
                Ok(sequence[index])
            },
            |(_, observation)| *observation,
        )?;
        Ok((value.0, calls))
    }

    #[test]
    fn a_faulted_observation_fails_on_the_first_attempt() -> Result<(), Box<dyn Error>> {
        let sequence = [(7, Observation::Faulted), (8, Observation::Counted)];
        assert_eq!(run(&sequence, LIMIT)?, (7, 1));
        Ok(())
    }

    #[test]
    fn a_not_yet_visible_observation_retries_until_it_is_counted() -> Result<(), Box<dyn Error>> {
        let sequence = [
            (1, Observation::NotYetVisible),
            (2, Observation::NotYetVisible),
            (3, Observation::Counted),
        ];
        assert_eq!(run(&sequence, LIMIT)?, (3, 3));
        Ok(())
    }

    #[test]
    fn a_never_visible_observation_returns_at_a_zero_deadline_without_retrying()
    -> Result<(), Box<dyn Error>> {
        // A zero limit is past its deadline on the first look, so the result
        // does not depend on how long the thread is descheduled.
        let sequence = [
            (1, Observation::NotYetVisible),
            (2, Observation::NotYetVisible),
        ];
        let (last, calls) = run(&sequence, Duration::ZERO)?;
        assert_eq!((last, calls), (1, 1));
        Ok(())
    }

    #[test]
    fn a_failed_observation_is_returned_without_retrying() {
        let mut calls = 0;
        let result: Result<u32, _> = poll_observation(
            LIMIT,
            INTERVAL,
            || {
                calls += 1;
                Err::<u32, Box<dyn Error>>("scan failed".into())
            },
            |_| Observation::NotYetVisible,
        );
        assert!(result.is_err());
        assert_eq!(calls, 1);
    }
}

//! Print counters from the next ordinary poll for one configured connection.
//!
//! Run with `<state-directory> <connection-id>`. No session or poll is created.

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use velnor_runner_host::{ConnectionCensus, ConnectionId, wait_for_connection_census};

fn main() -> ExitCode {
    let mut arguments = env::args_os().skip(1);
    let Some(state) = arguments.next() else {
        return usage();
    };
    let Some(connection) = arguments.next() else {
        return usage();
    };
    if arguments.next().is_some() {
        return usage();
    }
    let Some(connection) = connection.to_str() else {
        return usage();
    };
    let Ok(connection) = ConnectionId::parse(connection) else {
        return usage();
    };
    match wait_for_connection_census(&PathBuf::from(state), &connection, Duration::from_secs(30)) {
        Ok(observation) => {
            report(&observation);
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("census_once: {error}");
            ExitCode::from(1)
        }
    }
}

fn report(census: &ConnectionCensus) {
    println!(
        "setup_revision={} setup_captured_at_unix_ms={}",
        census.setup.revision, census.setup.captured_at_unix_ms
    );
    for group in &census.setup.runner_groups {
        println!(
            "runner_group id={} default={} name={:?}",
            group.id, group.is_default, group.name
        );
    }
    let set = &census.setup.selected_set;
    println!(
        "scale_set id={} runner_group_id={} disable_update={} name={:?}",
        set.id, set.runner_group_id, set.disable_update, set.name
    );
    for label in &set.labels {
        println!("scale_set_label={label:?}");
    }
    let observation = &census.observation;
    println!(
        "observed_at_unix_ms={} available_events={} statistics_present={}",
        observation.observed_at_unix_ms,
        observation.available_events,
        observation.statistics.is_some()
    );
    if let Some(stats) = observation.statistics {
        println!(
            "available_jobs={} acquired_jobs={} assigned_jobs={} running_jobs={} registered_runners={} busy_runners={} idle_runners={}",
            stats.available_jobs,
            stats.acquired_jobs,
            stats.assigned_jobs,
            stats.running_jobs,
            stats.registered_runners,
            stats.busy_runners,
            stats.idle_runners
        );
    }
}

fn usage() -> ExitCode {
    eprintln!("usage: census_once <state-directory> <connection-id>");
    ExitCode::from(2)
}

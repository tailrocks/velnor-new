//! Print the controller's cached Scale Set setup snapshot for one connection.
//!
//! Run with `<state-directory> <connection-id>`. This example does not run
//! setup or contact the remote service.

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use velnor_runner_host::{ConnectionId, ConnectionSetupReceipt, wait_for_connection_setup_receipt};

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
    match wait_for_connection_setup_receipt(
        &PathBuf::from(state),
        &connection,
        Duration::from_secs(30),
    ) {
        Ok(receipt) => {
            report(&receipt);
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("ensure_set: {error}");
            ExitCode::from(1)
        }
    }
}

fn report(receipt: &ConnectionSetupReceipt) {
    println!(
        "setup_revision={} setup_captured_at_unix_ms={}",
        receipt.revision, receipt.captured_at_unix_ms
    );
    for group in &receipt.runner_groups {
        println!(
            "runner_group id={} default={} name={:?}",
            group.id, group.is_default, group.name
        );
    }
    let set = &receipt.selected_set;
    println!(
        "scale_set id={} runner_group_id={} disable_update={} name={:?}",
        set.id, set.runner_group_id, set.disable_update, set.name
    );
    for label in &set.labels {
        println!("scale_set_label={label:?}");
    }
}

fn usage() -> ExitCode {
    eprintln!("usage: ensure_set <state-directory> <connection-id>");
    ExitCode::from(2)
}

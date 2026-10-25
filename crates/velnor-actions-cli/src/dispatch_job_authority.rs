use std::process::ExitCode;

use velnor_actions_orchestrator::{generate_job_authority_inventory, prepare, resolve_root};

use crate::dispatch::{fail_public, working_dir};

/// Write one authority inventory to standard output.
pub(crate) fn run() -> ExitCode {
    let Some(cwd) = working_dir() else {
        return ExitCode::from(1);
    };
    let root = match resolve_root(&cwd) {
        Ok(root) => root,
        Err(error) => return fail_public(&error),
    };
    let preparation = match prepare(&root) {
        Ok(preparation) => preparation,
        Err(error) => return fail_public(&error),
    };
    let inventory = match generate_job_authority_inventory(&preparation) {
        Ok(inventory) => inventory,
        Err(error) => return fail_public(&error),
    };
    if let Ok(json) = serde_json::to_string(&inventory) {
        println!("{json}");
        ExitCode::SUCCESS
    } else {
        eprintln!("velnor-actions: job_authority_inventory:encode_failed");
        ExitCode::from(1)
    }
}

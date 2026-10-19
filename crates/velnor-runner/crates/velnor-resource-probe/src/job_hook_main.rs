//! Run the hook before the official Runner starts a job.

mod job_hook;

use std::{io::Write, process};

fn main() {
    if let Err(error) = job_hook::run() {
        let message = error.to_string();
        let _write_result = writeln!(std::io::stderr().lock(), "job hook: {message}");
        process::exit(1);
    }
}

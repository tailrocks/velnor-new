//! Poll `ubuntu-26.04-scale-set` once, then delete the session.
//! The PAT comes from `gh auth token` and is not printed.

use std::process::{Command, ExitCode};

use velnor_runner_host::probe_once;
use zeroize::Zeroize;

fn main() -> ExitCode {
    let mut pat = match read_pat() {
        Ok(pat) => pat,
        Err(code) => return code,
    };
    let outcome = probe_once(&pat, "tailrocks", "velnor-new");
    pat.zeroize();
    match outcome {
        Ok(probe) => {
            println!("set_id={} available={}", probe.set_id, probe.available);
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("probe_once: {err}");
            ExitCode::from(1)
        }
    }
}

fn read_pat() -> Result<String, ExitCode> {
    let output = Command::new("gh")
        .args(["auth", "token"])
        .output()
        .map_err(|_| ExitCode::from(1))?;
    if !output.status.success() {
        return Err(ExitCode::from(1));
    }
    let text = String::from_utf8(output.stdout).map_err(|_| ExitCode::from(1))?;
    let pat = text.trim().to_owned();
    if pat.is_empty() {
        Err(ExitCode::from(1))
    } else {
        Ok(pat)
    }
}

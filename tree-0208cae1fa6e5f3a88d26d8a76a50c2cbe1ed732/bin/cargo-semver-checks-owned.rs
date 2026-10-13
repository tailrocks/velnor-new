#![forbid(unsafe_code)]

use anyhow::{Context, bail};
use cargo_semver_checks::{GlobalConfig, SuppliedCompareRequest, SuppliedPlanRequest};
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .map_err(|_| io::Error::other("native output lock poisoned"))?
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Capture {
    fn text(&self) -> anyhow::Result<String> {
        let bytes = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("native output lock poisoned"))?
            .clone();
        Ok(String::from_utf8(bytes)?)
    }
}

fn execute() -> anyhow::Result<bool> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() == 1 && args[0] == "--version" {
        println!("cargo-semver-checks-owned 0.50.0 velnor-supplied-v1");
        return Ok(true);
    }
    if args.len() != 2 {
        bail!("expected plan|compare REQUEST.json");
    }
    let bytes = std::fs::read(PathBuf::from(&args[1])).context("read owned request")?;
    let mut config = GlobalConfig::new();
    config.set_stdout(Box::new(std::io::stderr()));
    config.set_color_choice(false);
    config.set_log_level(Some(log::Level::Info));
    let (output, success) = match args[0].to_str() {
        Some("plan") => {
            let request: SuppliedPlanRequest = serde_json::from_slice(&bytes)?;
            let plan = cargo_semver_checks::plan_supplied(&mut config, &request)?;
            (serde_json::to_value(plan)?, true)
        }
        Some("compare") => {
            let request: SuppliedCompareRequest = serde_json::from_slice(&bytes)?;
            let capture = Capture::default();
            config.set_stdout(Box::new(capture.clone()));
            config.set_out_color_choice(false);
            let report = cargo_semver_checks::compare_supplied(&mut config, &request)?;
            let mut projection = cargo_semver_checks::supplied_report(&report);
            projection["native_report_stdout"] = capture.text()?.into();
            (projection, report.success())
        }
        _ => bail!("expected plan|compare REQUEST.json"),
    };
    println!("{}", serde_json::to_string(&output)?);
    Ok(success)
}

fn main() {
    let code = match execute() {
        Ok(true) => 0,
        Ok(false) => 100,
        Err(error) => {
            eprintln!("{error:#}");
            101
        }
    };
    std::process::exit(code);
}

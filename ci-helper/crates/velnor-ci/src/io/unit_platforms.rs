// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::Path;
use std::process::Command;

use crate::coverage::{Built, HOST};
use crate::error::CiError;
use crate::platform::{Platform, Platforms};

fn rustc() -> OsString {
    ["RUSTC", "CARGO_BUILD_RUSTC"]
        .iter()
        .find_map(|name| std::env::var_os(name).filter(|value| !value.is_empty()))
        .unwrap_or_else(|| OsString::from("rustc"))
}

pub(super) fn platforms(root: &Path, built: &Built) -> Result<Platforms, CiError> {
    let targets = built
        .units
        .iter()
        .filter_map(|unit| unit.configuration.strip_suffix(&format!("/{HOST}")))
        .filter(|directory| built.placed.contains(*directory))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|triple| {
            Command::new(rustc())
                .args(["--print", "cfg", "--target", triple])
                .current_dir(root)
                .output()
                .ok()
                .filter(|output| output.status.success())
                .map_or_else(Platform::unknown, |output| {
                    Platform::parse(triple, &String::from_utf8_lossy(&output.stdout))
                })
        })
        .collect();
    Ok(Platforms {
        host: host_platform(root)?,
        targets,
    })
}

fn host_platform(root: &Path) -> Result<Platform, CiError> {
    let run = |args: &[&str]| -> Result<String, CiError> {
        let output = Command::new(rustc())
            .args(args)
            .current_dir(root)
            .output()
            .map_err(|e| CiError::new(format!("cannot run rustc: {e}")))?;
        if !output.status.success() {
            return Err(CiError::new(format!(
                "rustc {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    };
    let (version, printed) = std::thread::scope(|scope| {
        let printed = scope.spawn(|| run(&["--print", "cfg"]));
        let version = run(&["-vV"]);
        (version, printed.join())
    });
    let version = version?;
    let triple = version
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .ok_or_else(|| CiError::new("rustc -vV printed no host triple"))?;
    let printed = printed.map_err(|_| CiError::new("rustc --print cfg panicked"))??;
    Ok(Platform::parse(triple, &printed))
}

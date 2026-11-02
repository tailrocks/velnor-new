// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

fn suite_job_env(text: &str) -> Result<BTreeMap<String, String>, Box<dyn Error>> {
    let jobs = text
        .split_once("\njobs:\n")
        .map(|(_, jobs)| jobs)
        .ok_or_else(|| fail("render lost its jobs section"))?;
    let after_env = jobs
        .split_once("\n    env:\n")
        .map(|(_, rest)| rest)
        .ok_or_else(|| fail("suite job lost its env block"))?;
    Ok(after_env
        .lines()
        .take_while(|line| line.starts_with("      "))
        .filter_map(|line| line.trim_start().split_once(": "))
        .map(|(key, value)| (key.to_owned(), value.trim_matches('"').to_owned()))
        .collect())
}

#[test]
fn gc1_t53_suite_jobs_run_the_compiler_with_shared_out_dirs() -> Result<(), Box<dyn Error>> {
    let env = suite_job_env(&render_text()?)?;
    for (key, expected) in [
        ("MBX_SHARE_OUT_DIR", "1"),
        ("MBX_BUILD_SCRIPT_EXECUTION", "1"),
        ("MBX_CC", "0"),
        ("MBX_GC_AUTO", "0"),
        ("MBX_CACHE_EXPORT_GROUP", "velnor"),
    ] {
        let actual = env.get(key).map(String::as_str);
        if actual != Some(expected) {
            return Err(fail(&format!(
                "suite job env {key} expected {expected:?}, got {actual:?}"
            )));
        }
    }
    Ok(())
}

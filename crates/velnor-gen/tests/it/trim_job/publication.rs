// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn target_publication_follows_its_test_build_and_input_audit() -> TestResult {
    for (config, id) in [("", "rust"), ("shape = \"lint-test\"\n", "rust-test")] {
        let order = names(&lane(config, id)?);
        let build = position(&order, "Build the tests")?;
        let save = position(&order, "Save the target cache")?;
        let run = position(&order, "Test")?;
        assert!(build < save, "{id}: {order:?}");
        if id == "rust" {
            assert!(
                run < save && position(&order, "velnor-ci inputs audit")? < save,
                "{order:?}"
            );
        } else {
            assert!(run < save, "{order:?}");
        }
    }
    let order = names(&lane("", "rust")?);
    for name in ["Lint", "Documentation"] {
        assert!(position(&order, name)? < position(&order, "Save the target cache")?);
    }
    Ok(())
}

fn resolved(text: &str) -> String {
    let mut text = text
        .replace("${{ runner.os }}", "Linux")
        .replace("${{ runner.arch }}", "X64")
        .replace("${{ steps.prepare.outputs.compat }}", "c0mpat");
    while let Some(start) = text.find("${{") {
        let end = text[start..]
            .find("}}")
            .map_or(text.len(), |end| start + end + 2);
        text.replace_range(start..end, "h");
    }
    text
}

fn target(job: &str, manifest: &str) -> Result<CacheKey, Box<dyn std::error::Error>> {
    let manifest = RepoPath::new(manifest)?;
    Ok(CacheKey::Target {
        job: JobId::new(job)?,
        root: CacheRoot::new(&manifest, RepoPath::new("target")?),
        host: Host,
    })
}

#[test]
fn no_job_and_root_pair_can_put_its_key_under_another_pairs_trim_prefix() -> TestResult {
    let mine = render_key(&target("rust", "a/Cargo.toml")?);
    let prefix = resolved(mine.restore_keys.last().ok_or("no prefix")?);
    let other = render_key(&target("rust-a-Linux-X64-c0mpat", "s/Cargo.toml")?);
    let key = resolved(&other.key);
    assert!(!key.starts_with(&prefix), "{key} is under {prefix}");
    let own = resolved(&mine.key);
    assert!(own.starts_with(&prefix), "{own} is not under {prefix}");
    Ok(())
}

fn permission_scopes(lines: &[&str], header: &str) -> Vec<String> {
    let Some(start) = lines.iter().position(|line| *line == header) else {
        return Vec::new();
    };
    let indent = header.len() - header.trim_start().len();
    lines[start + 1..]
        .iter()
        .take_while(|line| line.len() - line.trim_start().len() > indent)
        .map(|line| line.trim().to_owned())
        .collect()
}

fn pull_request_writers(text: &str) -> Vec<String> {
    let (header, _) = text.split_once("\njobs:\n").unwrap_or((text, ""));
    let head: Vec<&str> = header.lines().collect();
    let admits = [
        "  pull_request:",
        "  pull_request_target:",
        "  merge_group:",
    ]
    .iter()
    .any(|trigger| head.contains(trigger));
    if !admits {
        return Vec::new();
    }
    let inherited = permission_scopes(&head, "permissions:");
    jobs_of(text)
        .into_iter()
        .filter(|(_, block)| {
            let lines: Vec<&str> = block.lines().collect();
            let own = lines.contains(&"    permissions:");
            let scopes = if own {
                permission_scopes(&lines, "    permissions:")
            } else {
                inherited.clone()
            };
            let guarded = block.lines().any(|line| {
                line.starts_with("    if: ")
                    && line.contains(GUARD)
                    && line.contains(EVENTS)
                    && !line.contains("== ''pull_request''")
                    && !line.contains("== ''pull_request_target''")
            });
            scopes.iter().any(|scope| scope == "actions: write") && !guarded
        })
        .map(|(id, _)| id)
        .collect()
}

#[test]
fn no_job_of_any_sample_that_can_run_on_a_pull_request_or_merge_group_holds_actions_write()
-> TestResult {
    let mut checked = 0;
    for entry in fs::read_dir(samples())? {
        let sample = entry?.path();
        let (files, _) = expected_with_notes(&load(&sample)?)?;
        for (path, text) in &files {
            checked += 1;
            let writers = pull_request_writers(text);
            assert!(
                writers.is_empty(),
                "{} {path}: {writers:?} can run on a pull request with actions: write",
                sample.display()
            );
        }
    }
    assert!(checked > 0, "{checked} workflows");
    let saving = crate::pipeline::workflow(&crate::base_save::pinned("0.1.81", "")?)?;
    assert!(saving.contains("rust-trim:"), "{saving}");
    assert!(pull_request_writers(&saving).is_empty(), "{saving}");
    Ok(())
}

#[test]
fn the_pull_request_check_flags_a_lane_that_holds_actions_write_and_passes_the_guarded_trim_job()
-> TestResult {
    let text = crate::pipeline::workflow(&crate::base_save::pinned("0.1.81", "")?)?;
    let rust_permissions = format!("  rust:\n    if: '${{{{ {ADMISSION} }}}}'\n    permissions:\n");
    let broken = text.replace(
        &format!("{rust_permissions}      actions: read\n      contents: read\n"),
        &format!("{rust_permissions}      actions: write\n"),
    );
    assert_ne!(broken, text);
    assert_eq!(pull_request_writers(&broken), ["rust"]);
    let unguarded = text.replace(&format!("({EVENTS})"), "true");
    assert_eq!(pull_request_writers(&unguarded), ["rust-trim"]);
    Ok(())
}

#[test]
fn a_run_that_saved_only_sources_still_admits_the_trim_job_and_trims_them() -> TestResult {
    let all = jobs("0.1.81", "shape = \"lint-test\"\n")?;
    let find = |id: &str| {
        all.iter()
            .find(|(job, _)| job == id)
            .map(|(_, block)| block.as_str())
            .ok_or("missing job")
    };
    let test = find("rust-test")?;
    let save = step(test, "Save the sources cache").ok_or("no sources save")?;
    let id = save
        .lines()
        .find_map(|line| line.trim().strip_prefix("id: sources_save_"))
        .ok_or("the sources save has no id")?;
    assert!(
        test.contains(&format!(
            "      sources_saved: '${{{{ steps.sources_save_{id}.outcome == ''success'' }}}}'\n"
        )),
        "{test}"
    );
    assert!(!find("rust-lint")?.contains("sources_saved"));
    let trim = find("rust-trim")?;
    let condition = job_if(trim);
    assert!(!condition.contains("needs."), "{condition}");
    let prune =
        step(trim, "Delete the older rust-test sources entries").ok_or("no sources trim")?;
    assert!(prune.contains("sources_prefix"), "{prune}");
    assert!(
        prune.contains("needs.rust-test.outputs.sources_saved == ''true''"),
        "{prune}"
    );
    assert!(!prune.contains("base_save"), "{prune}");
    let budget = step(trim, "Enforce the repository cache budget").ok_or("no budget")?;
    assert!(!budget.contains("    if:"), "{budget}");
    Ok(())
}

use std::env;
use std::process::Command;

use super::{
    NEW_SCOPE_HASH, SHARED_SCOPE_HASH, SOURCE_SHA, cache_key_fixture_for_arch, observer_script,
};

fn run_api_script(
    root: &std::path::Path,
    runner_temp: &std::path::Path,
    jobs_path: &std::path::Path,
) -> std::process::Output {
    run_api_script_for_platform(root, runner_temp, jobs_path, "Linux", "X64", "x64")
}

fn run_api_script_for_platform(
    root: &std::path::Path,
    runner_temp: &std::path::Path,
    jobs_path: &std::path::Path,
    runner_os: &str,
    runner_arch: &str,
    key_arch: &str,
) -> std::process::Output {
    run_api_script_for_platform_with_duplicate_jobs(
        root,
        runner_temp,
        jobs_path,
        runner_os,
        runner_arch,
        key_arch,
        false,
    )
}

fn run_api_script_for_platform_with_duplicate_jobs(
    root: &std::path::Path,
    runner_temp: &std::path::Path,
    jobs_path: &std::path::Path,
    runner_os: &str,
    runner_arch: &str,
    key_arch: &str,
    duplicate_jobs: bool,
) -> std::process::Output {
    let fake_bin = root.join("bin");
    let seed_cache_path = root.join("seed-cache.json");
    let new_cache_path = root.join("new-cache.json");
    let script = observer_script();
    let mut path_entries = vec![fake_bin];
    path_entries.extend(env::split_paths(&env::var_os("PATH").unwrap_or_default()));
    let path = env::join_paths(path_entries).expect("join fixture PATH");
    let mut command = Command::new("bash");
    command.args(["-c", &script]).env("PATH", path);
    set_api_script_identity(&mut command, runner_temp);
    set_observer_cache_inputs(&mut command, key_arch);
    set_stock_fixture_inputs(&mut command, runner_temp, duplicate_jobs);
    set_parallel_probe_pins(&mut command, runner_os, runner_arch);
    set_parallel_fixture_inputs(
        &mut command,
        jobs_path,
        &seed_cache_path,
        &new_cache_path,
        key_arch,
    );
    command
        .output()
        .expect("run API receipt script against fixtures")
}

fn set_observer_cache_inputs(command: &mut Command, key_arch: &str) {
    command
        .env("GITHUB_RUN_ID", "123")
        .env(
            "MBX_PARALLEL_OBSERVER_PRIMARY",
            cache_key_fixture_for_arch(SHARED_SCOPE_HASH, key_arch),
        )
        .env(
            "MBX_PARALLEL_OBSERVER_RESTORE_PRIMARY",
            cache_key_fixture_for_arch(SHARED_SCOPE_HASH, key_arch),
        )
        .env("MBX_PARALLEL_OBSERVER_CACHE_HIT", "true")
        .env(
            "MBX_PARALLEL_OBSERVER_MATCHED_KEY",
            cache_key_fixture_for_arch(SHARED_SCOPE_HASH, key_arch),
        )
        .env("MBX_PARALLEL_OBSERVER_RESTORE_CONCLUSION", "success");
}

fn set_stock_fixture_inputs(
    command: &mut Command,
    runner_temp: &std::path::Path,
    duplicate_jobs: bool,
) {
    let fixture_root = runner_temp.parent().expect("fixture root");
    command
        .env("MBX_STOCK_FIXTURE_RUN", fixture_root.join("stock-run.json"))
        .env(
            "MBX_STOCK_FIXTURE_JOBS",
            fixture_root.join("stock-jobs.json"),
        )
        .env(
            "MBX_STOCK_DUPLICATE_JOBS",
            if duplicate_jobs { "true" } else { "false" },
        )
        .env("MBX_STOCK_FIXTURE_LOG_DIR", fixture_root);
}

fn set_parallel_probe_pins(command: &mut Command, runner_os: &str, runner_arch: &str) {
    command
        .env("RUNNER_OS", runner_os)
        .env("RUNNER_ARCH", runner_arch)
        .env(
            "MBX_EXPECTED_ACTION_REF",
            format!("jdx/mr-boxington-action@{}", "c".repeat(40)),
        )
        .env("MBX_EXPECTED_VERSION", "1.22.0")
        .env("MBX_EXPECTED_RUST_VERSION", "1.98.1")
        .env(
            "MBX_EXPECTED_SHARED_SCOPE",
            "qualification-mbx-v1/parallel/shared",
        )
        .env(
            "MBX_EXPECTED_NEW_KEY_SCOPE",
            "qualification-mbx-v1/parallel/new-key",
        );
}

fn set_parallel_fixture_inputs(
    command: &mut Command,
    jobs_path: &std::path::Path,
    seed_cache_path: &std::path::Path,
    new_cache_path: &std::path::Path,
    key_arch: &str,
) {
    command
        .env("MBX_PARALLEL_FIXTURE_JOBS", jobs_path)
        .env("MBX_PARALLEL_FIXTURE_SEED_CACHE", seed_cache_path)
        .env("MBX_PARALLEL_FIXTURE_NEW_CACHE", new_cache_path)
        .env("RUSTUP_TOOLCHAIN", "1.98.1")
        .env(
            "MBX_PARALLEL_FIXTURE_SEED_KEY",
            cache_key_fixture_for_arch(SHARED_SCOPE_HASH, key_arch),
        )
        .env(
            "MBX_PARALLEL_FIXTURE_NEW_KEY",
            cache_key_fixture_for_arch(NEW_SCOPE_HASH, key_arch),
        );
}

fn set_api_script_identity(command: &mut Command, runner_temp: &std::path::Path) {
    command
        .env("RUNNER_TEMP", runner_temp)
        .env("GITHUB_RUN_ATTEMPT", "2")
        .env("GITHUB_EVENT_NAME", "workflow_dispatch")
        .env("GITHUB_REF", "refs/heads/main")
        .env("GITHUB_REF_PROTECTED", "true")
        .env("GITHUB_REPOSITORY", "tailrocks/velnor-new")
        .env("GITHUB_EVENT_PATH", runner_temp.join("event.json"))
        .env(
            "GITHUB_WORKFLOW_REF",
            "tailrocks/velnor-new/.github/workflows/qualification.yml@refs/heads/main",
        )
        .env("GITHUB_SHA", SOURCE_SHA)
        .env("GH_TOKEN", "fixture-token");
}

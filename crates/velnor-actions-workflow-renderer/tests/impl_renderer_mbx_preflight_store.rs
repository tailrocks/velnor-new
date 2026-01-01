#![cfg(unix)]

use std::fs;
use std::os::unix::fs::symlink;
use std::process::Command;

use super::{TempRoot, preflight_script, write_tool};

#[test]
fn preflight_rejects_store_and_ancestor_symlinks_without_touching_targets()
-> Result<(), Box<dyn std::error::Error>> {
    for location in ["runner-temp", "velnor-root", "store-root"] {
        let root = TempRoot::new()?;
        let external = root.0.join("external");
        let fake_bin = root.0.join("fake-bin");
        fs::create_dir(&external)?;
        fs::create_dir(&fake_bin)?;
        fs::write(external.join("sentinel"), "untouched")?;
        write_tool(&fake_bin.join("mise"), "#!/bin/sh\nexit 99\n")?;
        let runner_temp = match location {
            "runner-temp" => {
                let link = root.0.join("runner-temp-link");
                symlink(&external, &link)?;
                link
            }
            _ => root.0.clone(),
        };
        if location == "velnor-root" {
            symlink(&external, runner_temp.join("velnor"))?;
        } else if location == "store-root" {
            fs::create_dir(runner_temp.join("velnor"))?;
            symlink(&external, runner_temp.join("velnor/mbx"))?;
        }
        let github_env = root.0.join("github-env");
        let github_path = root.0.join("github-path");
        fs::write(&github_env, "")?;
        fs::write(&github_path, "")?;
        let script = preflight_script()?;
        let result = Command::new("sh")
            .args(["-c", script.as_str()])
            .env_clear()
            .env("PATH", format!("{}:/usr/bin:/bin", fake_bin.display()))
            .env("RUNNER_TEMP", &runner_temp)
            .env("MBX_CACHE_DIR", runner_temp.join("velnor/mbx"))
            .env("GITHUB_RUN_ID", "76543")
            .env("GITHUB_RUN_ATTEMPT", "1")
            .env("GITHUB_PATH", &github_path)
            .env("GITHUB_ENV", &github_env)
            .env("MISE_RUSTUP_HOME", root.0.join("rustup"))
            .env("MISE_CARGO_HOME", root.0.join("cargo"))
            .env("RUSTUP_HOME", root.0.join("rustup"))
            .env("CARGO_HOME", root.0.join("cargo"))
            .output()?;
        assert!(
            !result.status.success(),
            "{location} symlink must fail closed"
        );
        assert_eq!(fs::read_to_string(external.join("sentinel"))?, "untouched");
        assert_eq!(fs::read_dir(&external)?.count(), 1);
        assert_eq!(fs::read_to_string(github_env)?, "");
        assert_eq!(fs::read_to_string(github_path)?, "");
    }
    Ok(())
}

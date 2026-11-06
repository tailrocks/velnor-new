// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

pub(super) fn committed_intent(
    root: &Path,
    source: &str,
    recovery: bool,
) -> Result<String, Box<dyn Error>> {
    let recovery = recovery
        .then(|| serde_json::json!({"package":"example","version":"0.1.1","source_sha":source}));
    let intent: velnor_actions_contract::config::CrateReleaseIntent = serde_json::from_value(
        serde_json::json!({"schema":1,"repository":"owner/project","default_branch":"main","environment":"release","required_workflow_path":".github/workflows/ci.yml","trusted_reviewer_id":139_017,"rust_version":"1.97.1","generator_version":"0.1.5","generator_source_sha":"f1041f322c54cd7edfdef06afd307799eeaa801a","generator_manifest_sha256":"e38d047150f9e0347ec2160fd65bb7f026842e6f416e456990cf3b7ccd284c52","release_plz_version":"0.4.3","runner":"ubuntu-26.04","authentication":"trusted-publishing","manifest_path":"Cargo.toml","pr_write_paths":["CHANGELOG.md","Cargo.lock","Cargo.toml"],"registry":"crates-io","packages":["example"],"publish_features":{"example":[]},"tag_template":"{{ package }}-v{{ version }}","recovery":recovery}),
    )?;
    std::fs::create_dir(root.join(".github"))?;
    std::fs::write(
        root.join(".github/velnor-release-intent.toml"),
        toml::to_string(&intent)?,
    )?;
    git(root, &["init", "-q"])?;
    git(root, &["add", "."])?;
    git(
        root,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            "fixture",
        ],
    )?;
    git(root, &["rev-parse", "HEAD"])
}

pub(super) fn git(root: &Path, args: &[&str]) -> Result<String, Box<dyn Error>> {
    let result = std::process::Command::new("git")
        .current_dir(root)
        .args(args)
        .output()?;
    if !result.status.success() {
        return Err(String::from_utf8_lossy(&result.stderr).into_owned().into());
    }
    Ok(String::from_utf8(result.stdout)?.trim().to_owned())
}

pub(super) fn source_require_script() -> Result<String, Box<dyn Error>> {
    generated_script("release-reconcile", "Release source-require")
}

pub(super) fn generated_script(job: &str, name: &str) -> Result<String, Box<dyn Error>> {
    let (spec, rust) = crate::release::release_steps::tests::fixture()?;
    let workflow = velnor_actions_workflow_renderer::release_tree::render_release_workflow(
        &spec,
        &velnor_actions_workflow_renderer::release_tree::ReleaseRenderContext {
            generator_version: env!("CARGO_PKG_VERSION").into(),
            runs_on: "ubuntu-26.04".into(),
        },
        &rust,
    )?;
    let workflow: serde_json::Value = serde_saphyr::from_str(&workflow)?;
    let steps = workflow["jobs"][job]["steps"]
        .as_array()
        .ok_or("publisher steps")?;
    Ok(steps
        .iter()
        .find(|step| step["name"].as_str() == Some(name))
        .and_then(|step| step["run"].as_str())
        .ok_or("source-require step")?
        .to_owned())
}

pub(super) fn serve_admission(
    listener: &std::net::TcpListener,
    done: &AtomicBool,
    policy: &str,
    source: &str,
    denied: bool,
) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut count = 0;
    while !done.load(Ordering::Acquire) && Instant::now() < deadline {
        let (mut stream, _) = match listener.accept() {
            Ok(connection) => connection,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(10));
                continue;
            }
            Err(error) => return Err(error.to_string()),
        };
        stream.set_nonblocking(false).map_err(|e| e.to_string())?;
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .map_err(|e| e.to_string())?;
        let (line, _) = super::super::tests::read_test_request(&mut stream)?;
        let mut value = if line == "GET /repos/owner/project HTTP/1.1" {
            serde_json::json!({"id":22,"full_name":"owner/project","owner":{"id":77,"login":"owner"},"default_branch":"main"})
        } else {
            super::super::tests::test_github_admission_response(&line, policy, source)
                .ok_or_else(|| format!("unexpected request {line}"))?
        };
        if denied && line.starts_with("GET /repos/owner/project/environments/release ") {
            value["can_admins_bypass"] = serde_json::json!(true);
        }
        let body = value.to_string();
        write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).map_err(|e| e.to_string())?;
        count += 1;
    }
    if count == 0 {
        return Err("no admission requests".into());
    }
    Ok(())
}

pub(super) fn supplied_step(
    root: &Path,
    job: &str,
    operation: &str,
) -> Result<std::process::Output, Box<dyn Error>> {
    let mut command = if job == "release-publish" {
        if !matches!(
            operation,
            "verify-admission" | "journal-start" | "journal-coordinate"
        ) {
            return Err("unexpected publisher operation".into());
        }
        let mut command = std::process::Command::new(std::env::current_exe()?);
        command.args(["--exact", super::CANDIDATE_CASE]);
        command.env(
            "VELNOR_TEST_COMMAND_ARGS",
            format!("crate-release {operation}"),
        );
        command
    } else {
        let script = generated_script(job, &format!("Release {operation}"))?;
        let mut command = std::process::Command::new("bash");
        command.args(["-e", "-c", &script]);
        command
    };
    command
        .env("VELNOR_NATIVE_RECEIPT_CHILD", "1")
        .env("VELNOR_NATIVE_OPERATION", operation)
        .env("VELNOR_TEST_BINARY", std::env::current_exe()?)
        .env("GITHUB_OUTPUT", root.join("github-output"))
        .env("RUSTUP_HOME", root.join("velnor/rustup"))
        .env("MISE_RUSTUP_HOME", root.join("velnor/rustup"))
        .env("CARGO_HOME", root.join("velnor/cargo"))
        .env("MISE_CARGO_HOME", root.join("velnor/cargo"))
        .env("RUSTUP_AUTO_INSTALL", "0");
    if matches!(operation, "journal-start" | "journal-coordinate") {
        command.env("HOME", root).env("GH_TOKEN", "fixture");
    }
    if operation == "journal-coordinate"
        && std::env::var_os("VELNOR_NATIVE_NO_TOKEN_ON_RETRY").is_some()
        && root.join("coordinator.pid").exists()
    {
        command.env_remove("ACTIONS_ID_TOKEN_REQUEST_TOKEN");
        command.env_remove("ACTIONS_ID_TOKEN_REQUEST_URL");
    }
    let output = command.output()?;
    if job == "release-publish" {
        assert!(String::from_utf8_lossy(&output.stdout).contains("running 1 test"));
    }
    Ok(output)
}

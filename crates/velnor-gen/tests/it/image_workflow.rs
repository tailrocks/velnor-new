// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;
use velnor_gen::expected_files;
use velnor_model::{RepoPath, RepoView};

use crate::facts::TestResult;
use crate::golden::{load, samples};
use crate::yaml_tree::document;

pub(crate) const SAMPLES: [&str; 3] = ["image-plain", "image-ghcr", "image-workspace"];
const WORKFLOW: &str = ".github/workflows/image.yml";
pub(crate) const CONFIG: &str = ".velnor/rust-ci.toml";

pub(crate) fn sample(name: &str) -> Result<RepoView, Box<dyn std::error::Error>> {
    load(&samples().join(name))
}

pub(crate) fn edited(
    name: &str,
    change: impl FnOnce(String) -> String,
) -> Result<RepoView, Box<dyn std::error::Error>> {
    let mut view = sample(name)?;
    let path = RepoPath::new(CONFIG)?;
    let text = String::from_utf8(view.files.remove(&path).ok_or("no config")?)?;
    view.files.insert(path, change(text).into_bytes());
    Ok(view)
}

pub(crate) fn image(view: &RepoView) -> Result<Value, Box<dyn std::error::Error>> {
    let mut files = expected_files(view)?;
    let text = files
        .remove(&RepoPath::new(WORKFLOW)?)
        .ok_or("no image workflow")?;
    Ok(document(&text)?)
}

pub(crate) fn rendered(name: &str) -> Result<Value, Box<dyn std::error::Error>> {
    image(&sample(name)?)
}

pub(crate) fn jobs(
    workflow: &Value,
) -> Result<&serde_json::Map<String, Value>, Box<dyn std::error::Error>> {
    Ok(workflow["jobs"].as_object().ok_or("no jobs")?)
}

pub(crate) fn steps(job: &Value) -> Vec<&Value> {
    job["steps"]
        .as_array()
        .map_or_else(Vec::new, |steps| steps.iter().collect())
}

pub(crate) fn step_named<'a>(
    job: &'a Value,
    name: &str,
) -> Result<&'a Value, Box<dyn std::error::Error>> {
    steps(job)
        .into_iter()
        .find(|step| {
            step["name"]
                .as_str()
                .is_some_and(|text| text.contains(name))
        })
        .ok_or_else(|| format!("no step named {name}").into())
}

pub(crate) fn uses_of<'a>(job: &'a Value, action: &str) -> Vec<&'a Value> {
    steps(job)
        .into_iter()
        .filter(|step| {
            step["uses"]
                .as_str()
                .is_some_and(|uses| uses.starts_with(action))
        })
        .collect()
}

#[test]
fn no_trigger_can_start_an_image_run_by_hand_or_from_another_workflow() -> TestResult {
    for name in SAMPLES {
        let workflow = rendered(name)?;
        let triggers: Vec<&str> = workflow["on"]
            .as_object()
            .ok_or("no triggers")?
            .keys()
            .map(String::as_str)
            .collect();
        assert!(
            triggers
                .iter()
                .all(|trigger| matches!(*trigger, "pull_request" | "push")),
            "{name}: {triggers:?}"
        );
        assert_eq!(workflow["permissions"], serde_json::json!({}), "{name}");
        let types = workflow["on"]["pull_request"]["types"].to_string();
        assert!(
            types.contains("edited"),
            "{name}: a retargeted pull request is checked again"
        );
        let build = workflow["jobs"]
            .as_object()
            .and_then(|jobs| jobs.iter().find(|(id, _)| id.ends_with("-build")))
            .map(|(_, job)| job["if"].to_string());
        assert!(
            build.is_some_and(|text| text.contains("github.event.changes.base != null")),
            "{name}"
        );
    }
    Ok(())
}

#[test]
fn registry_credentials_reach_only_the_publish_and_promote_jobs() -> TestResult {
    for name in SAMPLES {
        let workflow = rendered(name)?;
        for (id, job) in jobs(&workflow)? {
            let text = job.to_string();
            let privileged = id.ends_with("-publish") || id.ends_with("-promote");
            assert_eq!(text.contains("DOCKERHUB"), privileged, "{name} {id}");
            assert_eq!(
                job["environment"] == "images",
                privileged,
                "{name} {id} environment"
            );
            assert_eq!(
                text.contains("docker/login-action"),
                privileged,
                "{name} {id} login"
            );
        }
    }
    Ok(())
}

#[test]
fn the_compile_job_is_unprivileged_and_runs_only_for_a_push_or_a_same_repository_pull_request()
-> TestResult {
    for name in SAMPLES {
        let workflow = rendered(name)?;
        let job = &workflow["jobs"][format!("image-{}-build", name_of(name))];
        assert_eq!(
            job["permissions"],
            serde_json::json!({"contents": "read"}),
            "{name}"
        );
        assert!(job.get("environment").is_none(), "{name}");
        let condition = job["if"].as_str().ok_or("no condition")?;
        assert!(
            condition.contains("github.ref == 'refs/heads/main'"),
            "{name}"
        );
        assert!(
            condition
                .contains("github.event.pull_request.head.repo.full_name == github.repository"),
            "{name}"
        );
        assert!(
            condition.contains("!endsWith(github.actor, '[bot]')"),
            "{name}"
        );
    }
    Ok(())
}

pub(crate) fn name_of(sample: &str) -> &'static str {
    match sample {
        "image-plain" => "plain-svc",
        "image-ghcr" => "cluster-svc",
        _ => "multi-bin-svc",
    }
}

#[test]
fn only_a_push_to_the_default_branch_of_this_repository_reaches_the_gate_and_the_publish_job()
-> TestResult {
    for name in SAMPLES {
        let workflow = rendered(name)?;
        for id in [
            "image-gate".to_owned(),
            format!("image-{}-publish", name_of(name)),
        ] {
            let part = id.as_str();
            let condition = workflow["jobs"][part]["if"]
                .as_str()
                .ok_or("no condition")?
                .to_owned();
            assert!(
                condition.contains("github.event_name == 'push'"),
                "{name} {part}"
            );
            assert!(
                condition.contains("github.ref == 'refs/heads/main'"),
                "{name} {part}"
            );
            assert!(
                condition.contains(&format!("github.repository == '{name}'")),
                "{name} {part}"
            );
            assert!(!condition.contains("pull_request"), "{name} {part}");
        }
    }
    Ok(())
}

#[test]
fn the_pushed_image_carries_the_commit_tags_the_source_labels_and_the_declared_attestations()
-> TestResult {
    for (name, attested) in [
        ("image-plain", false),
        ("image-ghcr", true),
        ("image-workspace", true),
    ] {
        let workflow = rendered(name)?;
        let publish = &workflow["jobs"][format!("image-{}-publish", name_of(name))];
        let push = step_named(publish, "Build and push")?;
        let with = &push["with"];
        assert_eq!(with["platforms"], "linux/amd64");
        assert_eq!(with["push"], true);
        let tags = with["tags"].as_str().ok_or("no tags")?;
        for line in tags.lines() {
            assert!(
                line.ends_with(":sha-${{ github.sha }}")
                    || (line.starts_with("${{ steps.tip.outputs.ready == 'true' && '")
                        && line.ends_with(":main' || '' }}")),
                "{line}"
            );
        }
        let labels = with["labels"].as_str().ok_or("no labels")?;
        assert!(labels.contains(
            "org.opencontainers.image.source=${{ github.server_url }}/${{ github.repository }}"
        ));
        assert!(labels.contains("org.opencontainers.image.revision=${{ github.sha }}"));
        if attested {
            assert_eq!(with["provenance"], "mode=max");
            assert_eq!(with["sbom"], true);
        } else {
            assert_eq!(with["provenance"], false);
            assert_eq!(with["sbom"], false);
        }
        assert!(with.get("cache-from").is_none() && with.get("cache-to").is_none());
    }
    Ok(())
}

#[test]
fn a_second_registry_adds_one_login_and_the_same_two_tags_while_a_single_one_has_one_login()
-> TestResult {
    let plain = rendered("image-plain")?;
    let both = rendered("image-ghcr")?;
    let logins =
        |workflow: &Value, id: &str| uses_of(&workflow["jobs"][id], "docker/login-action").len();
    assert_eq!(logins(&plain, "image-plain-svc-publish"), 1);
    assert_eq!(logins(&both, "image-cluster-svc-publish"), 2);
    let publish = &both["jobs"]["image-cluster-svc-publish"];
    let tags = step_named(publish, "Build and push")?["with"]["tags"]
        .as_str()
        .ok_or("no tags")?
        .to_owned();
    assert_eq!(tags.lines().count(), 4);
    assert!(tags.contains("ghcr.io/acme/cluster-svc:sha-${{ github.sha }}"));
    let permissions = &publish["permissions"];
    assert_eq!(permissions["packages"], "write");
    assert!(
        plain["jobs"]["image-plain-svc-publish"]["permissions"]
            .get("packages")
            .is_none()
    );
    Ok(())
}

#[test]
fn a_release_tag_promotes_the_pushed_digest_and_never_builds() -> TestResult {
    for name in SAMPLES {
        let workflow = rendered(name)?;
        let promote = &workflow["jobs"][format!("image-{}-promote", name_of(name))];
        assert!(
            uses_of(promote, "docker/build-push-action").is_empty(),
            "{name}"
        );
        let create = steps(promote)
            .into_iter()
            .filter(|step| {
                step["run"]
                    .as_str()
                    .is_some_and(|run| run.starts_with("docker buildx imagetools create --tag"))
            })
            .count();
        let registries = if name == "image-ghcr" { 2 } else { 1 };
        assert_eq!(create, registries, "{name}");
        let authority = &workflow["jobs"][format!("image-{}-authority", name_of(name))];
        let condition = authority["if"].as_str().ok_or("no condition")?;
        assert!(
            condition.contains("startsWith(github.ref, 'refs/tags/"),
            "{name}"
        );
        assert_eq!(
            promote["needs"],
            serde_json::json!([format!("image-{}-authority", name_of(name))])
        );
    }
    Ok(())
}

#[test]
fn a_private_docker_hub_repository_is_checked_before_login_and_a_ghcr_only_publish_has_no_check()
-> TestResult {
    let workflow = rendered("image-ghcr")?;
    let publish = &workflow["jobs"]["image-cluster-svc-publish"];
    let order: Vec<&str> = steps(publish)
        .iter()
        .filter_map(|step| step["name"].as_str())
        .collect();
    let check = order
        .iter()
        .position(|name| name.starts_with("Refuse to push"))
        .ok_or("no preflight")?;
    let login = order
        .iter()
        .position(|name| name.starts_with("Log in"))
        .ok_or("no login")?;
    assert!(check < login);
    let edited = edited("image-plain", |text| {
        text.replace(
            "token = \"secrets.DOCKERHUB_TOKEN\"\n",
            "token = \"secrets.DOCKERHUB_TOKEN\"\nvisibility = \"public\"\n",
        )
    })?;
    let public = image(&edited)?;
    let publish = &public["jobs"]["image-plain-svc-publish"];
    assert!(steps(publish).iter().all(|step| {
        !step["name"]
            .as_str()
            .unwrap_or("")
            .starts_with("Refuse to push")
    }));
    Ok(())
}

#[test]
fn the_compile_step_names_the_glibc_target_the_selection_and_the_staged_directory() -> TestResult {
    let ghcr = rendered("image-ghcr")?;
    let build = &ghcr["jobs"]["image-cluster-svc-build"];
    assert_eq!(
        step_named(build, "Compile")?["run"],
        ". \"$VELNOR_EXEC\" run -- cargo zigbuild --release --locked --offline --target-dir target --target x86_64-unknown-linux-gnu.2.41 --package cluster-svc --bin cluster-svc"
    );
    assert_eq!(
        step_named(build, "Stage the binaries")?["run"],
        ". \"$VELNOR_EXEC\" image stage --target-dir target --profile release --glibc 2.41 --openssl none --to dist --bin cluster-svc"
    );
    let workspace = rendered("image-workspace")?;
    let build = &workspace["jobs"]["image-multi-bin-svc-build"];
    let compile = step_named(build, "Compile")?;
    assert_eq!(compile["working-directory"], "backend-rust");
    assert_eq!(
        compile["run"],
        ". \"$VELNOR_EXEC\" run -- cargo zigbuild --release --locked --target-dir target --target x86_64-unknown-linux-gnu.2.36 --package legacy-server --package legacy-migrate --bin legacy-server --bin legacy-migration --features tls"
    );
    assert_eq!(
        step_named(build, "Stage the binaries")?["run"],
        ". \"$VELNOR_EXEC\" image stage --target-dir backend-rust/target --profile release --glibc 2.36 --openssl sysroot --to dist --bin legacy-server --bin legacy-migration"
    );
    assert!(
        step_named(build, "OpenSSL sysroot")?["run"]
            .as_str()
            .is_some_and(|run| run.contains("--lock .velnor/image-sysroot.lock"))
    );
    Ok(())
}

#[test]
fn the_publish_job_checks_out_only_the_dockerfile_and_its_inputs_and_downloads_the_binaries()
-> TestResult {
    let ghcr = rendered("image-ghcr")?;
    let publish = &ghcr["jobs"]["image-cluster-svc-publish"];
    let checkout = &uses_of(publish, "actions/checkout")[0]["with"];
    assert_eq!(
        checkout["sparse-checkout"],
        "/Dockerfile\n/Dockerfile.dockerignore\n/.dockerignore\n/migrations\n"
    );
    assert_eq!(checkout["sparse-checkout-cone-mode"], false);
    assert_eq!(checkout["persist-credentials"], false);
    let download = &uses_of(publish, "actions/download-artifact")[0]["with"];
    assert_eq!(download["name"], "image-cluster-svc");
    assert_eq!(download["path"], "dist");
    Ok(())
}

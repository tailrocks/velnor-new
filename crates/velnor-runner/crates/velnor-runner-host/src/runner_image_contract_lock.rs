use super::{
    DIND_DOCKERFILE, DIND_README, GNU_TAR_BUILD_SCRIPT, GNU_TAR_SOURCE_LOCK, RESOURCE_PROBE_BUILD,
    RUNNER_DOCKERFILE, RUNNER_README, dind_create,
};

fn lock_str<'a>(lock: &'a toml::Value, key: &str) -> &'a str {
    lock.get(key)
        .and_then(toml::Value::as_str)
        .unwrap_or_else(|| panic!("runner.lock is missing string field {key}"))
}

fn docker_arg<'a>(dockerfile: &'a str, name: &str) -> Option<&'a str> {
    let prefix = format!("ARG {name}=");
    dockerfile
        .lines()
        .find_map(|line| line.strip_prefix(prefix.as_str()))
}

pub(super) fn assert_runner_release_lock(lock: &toml::Value) {
    assert_eq!(lock_str(lock, "platform"), "linux/amd64");
    assert_eq!(lock_str(lock, "runner_platform"), "linux/amd64");
    assert_eq!(lock_str(lock, "runner_release_channel"), "stable");
    assert_eq!(lock_str(lock, "runner_release_observed_date"), "2026-10-18");
    assert_eq!(lock_str(lock, "runner_version"), "2.338.0");
    assert_eq!(
        lock_str(lock, "runner_source_commit"),
        "197ca027b90de199464dee3bb9ff198a1db60967"
    );
    assert_eq!(
        lock_str(lock, "runner_sha256"),
        "af4b794c1bc41d73d40535e3fe092a39f9679cd8d965954c2aca25a05ca41d32"
    );
    assert_eq!(lock_str(lock, "runner_scope_download_version"), "2.337.0");
    assert_ne!(
        lock_str(lock, "runner_scope_download_version"),
        lock_str(lock, "runner_version")
    );
}

pub(super) fn assert_runtime_component_lock(lock: &toml::Value) {
    assert_eq!(lock_str(lock, "docker_version"), "29.9.1");
    assert_eq!(
        lock_str(lock, "docker_sha256"),
        "c158ecc55adb3afb01bbd3539ed3b997d20c26dbdc418080506cc1bf5243ff57"
    );
    assert_eq!(lock_str(lock, "buildx_version"), "0.38.0");
    assert_eq!(
        lock_str(lock, "buildx_sha256"),
        "4fe4cc38adf48169132749b6ca22a990928db0118e3407584ee553723115d287"
    );
    assert_eq!(lock_str(lock, "compose_version"), "5.6.0");
    assert_eq!(
        lock_str(lock, "compose_sha256"),
        "40343e21ca777173e69cff5dbafeb37c6f81f3b0d57d9e597f036e95eb63e76a"
    );
    assert_eq!(lock_str(lock, "node_version"), "26.12.0");
    assert_eq!(lock_str(lock, "node_release_channel"), "Current");
    assert_eq!(
        lock_str(lock, "node_sha256"),
        "d9f88af72ad34f7ddf8e67289829f1698f54b6254f7d4f54b7453117ff1a3d2c"
    );
    assert_eq!(lock_str(lock, "turso"), "=0.8.3");
}

pub(super) fn assert_pinned_base_image(lock: &toml::Value) {
    let base = format!(
        "FROM {}@{}",
        lock_str(lock, "base_image"),
        lock_str(lock, "base_image_digest")
    );
    assert!(RUNNER_DOCKERFILE.lines().any(|line| {
        line == base.as_str()
            || line
                .strip_prefix(base.as_str())
                .is_some_and(|suffix| suffix.starts_with(" AS "))
    }));
    assert!(DIND_DOCKERFILE.lines().any(|line| line == base.as_str()));
    assert_eq!(
        lock_str(lock, "base_image_index_digest"),
        "sha256:c098726dd9b9733f3e885f8de0ef375206a311b0a317eadc40d347b1115fb944"
    );
}

pub(super) fn assert_dockerfile_component_args(lock: &toml::Value) {
    for (dockerfile, arg, field) in [
        (&RUNNER_DOCKERFILE, "RUNNER_VERSION", "runner_version"),
        (&RUNNER_DOCKERFILE, "RUNNER_SHA256", "runner_sha256"),
        (&RUNNER_DOCKERFILE, "DOCKER_VERSION", "docker_version"),
        (&RUNNER_DOCKERFILE, "DOCKER_SHA256", "docker_sha256"),
        (&RUNNER_DOCKERFILE, "COMPOSE_VERSION", "compose_version"),
        (&RUNNER_DOCKERFILE, "COMPOSE_SHA256", "compose_sha256"),
        (&RUNNER_DOCKERFILE, "BUILDX_VERSION", "buildx_version"),
        (&RUNNER_DOCKERFILE, "BUILDX_SHA256", "buildx_sha256"),
        (&RUNNER_DOCKERFILE, "NODE_VERSION", "node_version"),
        (&RUNNER_DOCKERFILE, "NODE_SHA256", "node_sha256"),
        (&DIND_DOCKERFILE, "DOCKER_VERSION", "docker_version"),
        (&DIND_DOCKERFILE, "DOCKER_SHA256", "docker_sha256"),
    ] {
        assert_eq!(
            docker_arg(dockerfile, arg),
            Some(lock_str(lock, field)),
            "{arg}"
        );
    }
}

pub(super) fn assert_runner_artifact_matches_lock(lock: &toml::Value) {
    assert_runner_release_payload(lock);
    assert_gnu_tar_build();
    assert_runner_payload_end_state();
}

fn assert_runner_release_payload(lock: &toml::Value) {
    let runner_tag = format!(
        "velnor-runner:ubuntu-26.04-{}",
        lock_str(lock, "runner_version")
    );
    assert_eq!(crate::docker_spec::RUNNER_IMAGE, runner_tag);
    assert!(RUNNER_README.contains(runner_tag.as_str()));
    assert!(RUNNER_README.contains(lock_str(lock, "runner_source_commit")));
    assert!(RUNNER_README.contains("not evidence that `2.338.0` is\nrejected"));
    assert!(RUNNER_DOCKERFILE.contains("actions-runner-linux-x64-${RUNNER_VERSION}.tar.gz"));
    let checksum_check = RUNNER_DOCKERFILE
        .find(r#"echo "${RUNNER_SHA256}  /tmp/actions-runner.tar.gz" | sha256sum -c -"#)
        .expect("Runner archive checksum is present");
    let runner_extract = RUNNER_DOCKERFILE
        .find("/usr/bin/tar -xzf /tmp/actions-runner.tar.gz -C /opt/runner")
        .expect("The Runner archive is extracted into the Runner seed with GNU tar.");
    let runner_owner_normalization = RUNNER_DOCKERFILE
        .find("chown -R root:root /opt/runner")
        .expect("Archive ownership is set before the Runner seed manifest.");
    let seed_manifest = RUNNER_DOCKERFILE
        .find("--output /tmp/runner-home-manifest.before.json")
        .expect("The Runner seed manifest is present.");
    assert!(checksum_check < runner_extract);
    assert!(runner_extract < runner_owner_normalization);
    assert!(runner_owner_normalization < seed_manifest);
    assert!(RUNNER_DOCKERFILE.contains("/opt/runner/bin/installdependencies.sh"));
    assert!(RUNNER_DOCKERFILE.contains("/tmp/runner-home-manifest.before.json"));
    assert!(RUNNER_DOCKERFILE.contains("/tmp/runner-home-manifest.after.json"));
    assert!(RUNNER_DOCKERFILE.contains(
        "cmp -s /tmp/runner-home-manifest.before.json /tmp/runner-home-manifest.after.json"
    ));
    assert!(RUNNER_DOCKERFILE.contains("/usr/share/velnor/runner-home-manifest.json"));
    assert!(
        RUNNER_DOCKERFILE
            .contains("COPY --from=resource_probe resource-probe /velnor/resource-probe")
    );
    assert!(
        RUNNER_DOCKERFILE.contains("python3 /tmp/velnor-runner-home-tests/runner-home-test.py")
    );
}

fn assert_gnu_tar_build() {
    assert!(RUNNER_DOCKERFILE.contains("AS gnu-tar-build"));
    assert!(RUNNER_DOCKERFILE.contains(
        "COPY build-gnu-tar.sh gnu-tar-source.lock.json gnu-tar-openat2-declaration.patch /build/"
    ));
    assert!(RUNNER_DOCKERFILE.contains("/build/build-gnu-tar.sh /out"));
    assert!(RUNNER_DOCKERFILE.contains("RUN --mount=type=tmpfs,target=/tmp,size=15032385536"));
    assert!(RUNNER_DOCKERFILE.contains("dpkg --print-architecture"));
    assert!(RUNNER_DOCKERFILE.contains("uname -m"));
    assert!(RUNNER_DOCKERFILE.contains("COPY --from=gnu-tar-build /out/usr/bin/tar /usr/bin/tar"));
    assert!(
        RUNNER_DOCKERFILE
            .contains("035ec786cea68c6da6065e8c8d78e9b08aed5bbd168dee4e52da4a56417ba4c6")
    );
    assert!(RUNNER_DOCKERFILE.contains("COPY tar-semantics-test.sh tar-semantics-planned-test.sh tar-absolute-test.sh tar-absolute-test-extended.sh"));
    assert!(RUNNER_README.contains("GNU tar 1.35"));
    assert!(RUNNER_README.contains("build-runner-images.sh"));
    assert!(RESOURCE_PROBE_BUILD.contains("--platform linux/amd64"));
    assert!(RESOURCE_PROBE_BUILD.contains("--resource memory=20g"));
    assert!(RESOURCE_PROBE_BUILD.contains("--resource cpu-quota=400000"));
    assert!(RUNNER_README.contains("14 GiB temporary filesystem at `/tmp`"));
    assert!(GNU_TAR_BUILD_SCRIPT.contains("dpkg --print-architecture"));
    assert!(GNU_TAR_BUILD_SCRIPT.contains("uname -m"));
    assert!(GNU_TAR_BUILD_SCRIPT.contains("build[\"required_tests\"]"));
    assert!(!GNU_TAR_BUILD_SCRIPT.contains("expected_test_results"));
    let tar_lock: serde_json::Value =
        serde_json::from_str(GNU_TAR_SOURCE_LOCK).expect("GNU tar source lock is valid JSON");
    assert_eq!(tar_lock["build"]["target"], "linux/amd64");
    assert_eq!(
        tar_lock["build"]["required_tests"],
        serde_json::json!([110, 111, 112, 178])
    );
    assert_eq!(tar_lock["build"]["resource_envelope"]["memory"], "20g");
    assert_eq!(tar_lock["build"]["resource_envelope"]["cpu_quota"], 400_000);
    assert_eq!(
        tar_lock["build"]["resource_envelope"]["temporary_filesystem"],
        "/tmp"
    );
    assert_eq!(
        tar_lock["build"]["resource_envelope"]["temporary_filesystem_size_bytes"],
        15_032_385_536_i64
    );
}

fn assert_runner_payload_end_state() {
    for obsolete_path in [
        "busybox tar",
        "dpkg-divert",
        "tar-shim.sh",
        "tar-absolute.sh",
        "tar-extract.sh",
        "tar-extract-plan.sh",
        "tar-dir-meta.pl",
        "tar-member.pl",
        "tar-member-rewrite.pl",
        "tar-member-stream.pl",
        "tar-pax.pl",
    ] {
        assert!(
            !RUNNER_DOCKERFILE.contains(obsolete_path),
            "obsolete tar path remains: {obsolete_path}"
        );
    }
    assert!(RUNNER_README.contains("Runner.Listener and its"));
    assert!(RUNNER_README.contains("embedded Node 24 runtime are not patched."));
    assert!(!RUNNER_DOCKERFILE.contains("externals/node24"));
}

pub(super) fn assert_dind_artifact_matches_lock(lock: &toml::Value) -> Result<(), String> {
    let dind_tag = format!("velnor-dind:{}", lock_str(lock, "docker_version"));
    assert!(DIND_README.contains(dind_tag.as_str()));
    assert!(DIND_README.contains(lock_str(lock, "docker_source_commit")));

    let resource_budget =
        crate::worker::test_resource_budget().map_err(|error| error.to_string())?;
    let runtime_plan =
        dind_create("runner-contract", resource_budget).map_err(|error| error.to_string())?;
    assert_eq!(runtime_plan.image, dind_tag);
    Ok(())
}

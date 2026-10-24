use std::fmt::Write as _;

use sha2::{Digest, Sha256};

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
    assert_gnu_tar_runtime_lock(lock);
}

fn assert_gnu_tar_runtime_lock(lock: &toml::Value) {
    let package_version = lock_str(lock, "gnu_tar_source_package_version");
    assert_eq!(package_version, "1.35+dfsg-4ubuntu0.4");
    assert_eq!(
        lock_str(lock, "gnu_tar_source_lock_sha256"),
        "110b375dcff4a8824a913fa55118773d39d017535d2a08aaae4d856bf5c906db"
    );
    let source_lock_sha256 = sha256_hex(GNU_TAR_SOURCE_LOCK.as_bytes());
    assert_eq!(
        lock_str(lock, "gnu_tar_source_lock_sha256"),
        source_lock_sha256
    );

    let tar_lock: serde_json::Value =
        serde_json::from_str(GNU_TAR_SOURCE_LOCK).expect("GNU tar source lock is valid JSON");
    assert_eq!(tar_lock["package"], "tar");
    assert_eq!(tar_lock["version"], package_version);

    let binary_sha256 = lock_str(lock, "gnu_tar_binary_sha256");
    assert_eq!(
        binary_sha256,
        "035ec786cea68c6da6065e8c8d78e9b08aed5bbd168dee4e52da4a56417ba4c6"
    );
    assert_eq!(tar_lock["runtime"]["tar_sha256"], binary_sha256);

    for expected_line in [
        format!(
            "RUN echo '{binary_sha256}  /usr/bin/tar' | sha256sum -c - {}",
            char::from(92_u8)
        ),
        format!("&& echo '{binary_sha256}  /usr/bin/tar' | sha256sum -c -"),
    ] {
        assert!(
            RUNNER_DOCKERFILE
                .lines()
                .any(|line| line.trim() == expected_line),
            "GNU tar checksum command does not match runner.lock: {expected_line}"
        );
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(&mut encoded, "{byte:02x}").expect("writing to String cannot fail");
    }
    encoded
}

pub(super) fn assert_pinned_base_image(lock: &toml::Value) {
    let image = format!(
        "{}@{}",
        lock_str(lock, "base_image_name"),
        lock_str(lock, "base_image_digest")
    );
    assert_eq!(lock_str(lock, "base_image"), image);
    let base = format!("FROM {image}");
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
        "tar-shim.sh",
        "tar-absolute.sh",
        "tar-extract.sh",
        "tar-extract-plan.sh",
        "tar-dir-meta.pl",
        "tar-pax.pl",
    ] {
        assert!(
            !RUNNER_DOCKERFILE.contains(obsolete_path),
            "obsolete tar path remains: {obsolete_path}"
        );
    }
    assert_guarded_tar_frontdoor();
    assert!(RUNNER_README.contains("Runner.Listener and its"));
    assert!(RUNNER_README.contains("embedded Node 24 runtime are not patched."));
    assert!(!RUNNER_DOCKERFILE.contains("externals/node24"));
}

fn assert_guarded_tar_frontdoor() {
    const TAR_SHA256: &str = "035ec786cea68c6da6065e8c8d78e9b08aed5bbd168dee4e52da4a56417ba4c6";
    let backend_checksum = format!("echo '{TAR_SHA256}  /usr/bin/tar.gnu' | sha256sum -c -");

    assert!(RUNNER_DOCKERFILE.contains(
        "RUN dpkg-divert --local --rename --divert /usr/bin/tar.ubuntu --add /usr/bin/tar"
    ));
    assert!(
        RUNNER_DOCKERFILE
            .contains("&& dpkg-divert --list /usr/bin/tar | grep -F /usr/bin/tar.ubuntu")
    );
    assert!(RUNNER_DOCKERFILE.contains("&& test -x /usr/bin/tar.ubuntu"));
    assert!(RUNNER_DOCKERFILE.contains("&& test ! -e /usr/bin/tar"));

    assert!(
        RUNNER_DOCKERFILE.contains("COPY --from=gnu-tar-build /out/usr/bin/tar /usr/bin/tar.gnu")
    );
    assert!(RUNNER_DOCKERFILE.contains(backend_checksum.as_str()));
    assert!(
        RUNNER_DOCKERFILE.contains("&& /usr/bin/tar.gnu --version | grep -F 'tar (GNU tar) 1.35'")
    );
    assert!(RUNNER_DOCKERFILE.contains("COPY tar-guard.sh /usr/local/bin/velnor-tar"));
    assert!(RUNNER_DOCKERFILE.contains(
        "COPY tar-guard-preflight.sh tar-member.pl tar-member-stream.pl tar-member-rewrite.pl /usr/local/libexec/velnor/"
    ));
    assert!(RUNNER_DOCKERFILE.contains(
        "&& chmod 0555 /usr/local/bin/velnor-tar /usr/local/libexec/velnor/tar-guard-preflight.sh"
    ));
    assert!(RUNNER_DOCKERFILE.contains(
        "&& chmod 0444 /usr/local/libexec/velnor/tar-member.pl /usr/local/libexec/velnor/tar-member-stream.pl /usr/local/libexec/velnor/tar-member-rewrite.pl"
    ));
    assert!(RUNNER_DOCKERFILE.contains("&& ln -s /usr/local/bin/velnor-tar /usr/bin/tar"));
    assert!(
        RUNNER_DOCKERFILE
            .contains("&& test \"$(readlink /usr/bin/tar)\" = /usr/local/bin/velnor-tar")
    );

    let backend_copy = RUNNER_DOCKERFILE
        .find("COPY --from=gnu-tar-build /out/usr/bin/tar /usr/bin/tar.gnu")
        .expect("Pinned GNU tar backend is installed at its native path");
    let backend_verify = RUNNER_DOCKERFILE
        .find(backend_checksum.as_str())
        .expect("Pinned GNU tar backend is verified before frontdoor setup");
    let wrapper_copy = RUNNER_DOCKERFILE
        .find("COPY tar-guard.sh /usr/local/bin/velnor-tar")
        .expect("Guard wrapper is installed before frontdoor activation");
    let preflight_copy = RUNNER_DOCKERFILE
        .find("COPY tar-guard-preflight.sh tar-member.pl tar-member-stream.pl tar-member-rewrite.pl /usr/local/libexec/velnor/")
        .expect("Guard preflight and member readers are installed");
    let frontdoor_link = RUNNER_DOCKERFILE
        .find("&& ln -s /usr/local/bin/velnor-tar /usr/bin/tar")
        .expect("Conventional tar frontdoor resolves to the guard");
    let native_suites = RUNNER_DOCKERFILE
        .find("TAR_BIN=/usr/bin/tar.gnu \"$test_file\" || exit")
        .expect("Native tar suites exercise the pinned backend directly");
    let guard_suite = RUNNER_DOCKERFILE
        .find("&& /tmp/velnor-gnu-tar-tests/tar-guard-test.sh")
        .expect("Guard behavior suite runs after native tar suites");
    assert!(
        backend_copy < backend_verify
            && backend_verify < frontdoor_link
            && wrapper_copy < frontdoor_link
            && preflight_copy < frontdoor_link
            && frontdoor_link < native_suites
            && native_suites < guard_suite,
        "pinned backend, guarded frontdoor, and tar suites are out of order"
    );
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

use super::{
    DIND_DOCKERFILE, DIND_README, RESOURCE_PROBE_BUILD, RUNNER_DOCKERFILE, RUNNER_ENTRYPOINT,
    RUNNER_HOME_INIT, RUNNER_HOME_TEST, RUNNER_README,
};

pub(super) fn assert_home_and_mount_contract() {
    assert_image_pins_and_runner_user();
    assert_entrypoint_and_home_contract();
    assert_runtime_and_work_contract();
}

fn assert_image_pins_and_runner_user() {
    assert!(RUNNER_DOCKERFILE.contains("ARG RUNNER_VERSION=2.338.0"));
    assert!(RUNNER_DOCKERFILE.contains(
        "ARG RUNNER_SHA256=af4b794c1bc41d73d40535e3fe092a39f9679cd8d965954c2aca25a05ca41d32"
    ));
    assert!(RUNNER_DOCKERFILE.contains("ARG DOCKER_VERSION=29.9.1"));
    assert!(RUNNER_DOCKERFILE.contains(
        "ARG DOCKER_SHA256=c158ecc55adb3afb01bbd3539ed3b997d20c26dbdc418080506cc1bf5243ff57"
    ));
    assert!(RUNNER_DOCKERFILE.contains("ARG COMPOSE_VERSION=5.6.0"));
    assert!(RUNNER_DOCKERFILE.contains(
        "ARG COMPOSE_SHA256=40343e21ca777173e69cff5dbafeb37c6f81f3b0d57d9e597f036e95eb63e76a"
    ));
    assert!(RUNNER_DOCKERFILE.contains("ARG BUILDX_VERSION=0.38.0"));
    assert!(RUNNER_DOCKERFILE.contains(
        "ARG BUILDX_SHA256=4fe4cc38adf48169132749b6ca22a990928db0118e3407584ee553723115d287"
    ));
    assert!(RUNNER_DOCKERFILE.contains("ARG NODE_VERSION=26.12.0"));
    assert!(RUNNER_DOCKERFILE.contains(
        "ARG NODE_SHA256=d9f88af72ad34f7ddf8e67289829f1698f54b6254f7d4f54b7453117ff1a3d2c"
    ));
    assert!(DIND_DOCKERFILE.contains("ARG DOCKER_VERSION=29.9.1"));
    assert!(DIND_DOCKERFILE.contains(
        "ARG DOCKER_SHA256=c158ecc55adb3afb01bbd3539ed3b997d20c26dbdc418080506cc1bf5243ff57"
    ));
    assert!(RUNNER_README.contains("velnor-runner:ubuntu-26.04-2.338.0"));
    assert!(RUNNER_README.contains("Official `actions/runner` `2.338.0`"));
    assert!(RUNNER_README.contains("Node.js `26.12.0`"));
    assert!(DIND_README.contains("velnor-dind:29.9.1"));
    assert!(RUNNER_DOCKERFILE.contains("groupadd --gid 1000 runner"));
    assert!(RUNNER_DOCKERFILE.contains("useradd --uid 1000 --gid 1000"));
    assert!(RUNNER_DOCKERFILE.contains("mkdir -p /home/runner/_work"));
    assert!(RUNNER_DOCKERFILE.contains("libacl1"));
    assert!(RUNNER_DOCKERFILE.contains("libselinux1"));
    assert!(!RUNNER_DOCKERFILE.contains("-C /home/runner"));
    assert!(RUNNER_DOCKERFILE.contains("WORKDIR /home/runner"));
    assert!(RUNNER_DOCKERFILE.contains("USER runner"));
    assert!(RUNNER_DOCKERFILE.contains("chown -R root:root /opt/runner"));
}

fn assert_entrypoint_and_home_contract() {
    assert!(RUNNER_ENTRYPOINT.contains("root=\"/home/runner\""));
    assert!(RUNNER_ENTRYPOINT.contains("work=\"${root}/_work\""));
    assert!(RUNNER_ENTRYPOINT.contains("listener=\"${root}/bin/Runner.Listener\""));
    assert!(RUNNER_ENTRYPOINT.contains("mktemp /tmp/velnor-jit.XXXXXX"));
    assert!(!RUNNER_ENTRYPOINT.contains("mktemp \"${work}/"));
    assert!(RUNNER_ENTRYPOINT.contains("python3 \"$home_initializer\" initialize"));
    assert!(RUNNER_ENTRYPOINT.contains("if [[ ! -d \"$work\" ]]"));
    let ambient_jit_check = RUNNER_ENTRYPOINT
        .find("if [[ \"${ACTIONS_RUNNER_INPUT_JITCONFIG+x}\" ]]")
        .expect("The ambient JIT input check is present.");
    let home_initialization = RUNNER_ENTRYPOINT
        .find("python3 \"$home_initializer\" initialize")
        .expect("The Runner home volume is ready before private input.");
    let listener_check = RUNNER_ENTRYPOINT
        .find("if [[ ! -x \"$listener\" ]]")
        .expect("The listener check is present.");
    let jit_read = RUNNER_ENTRYPOINT
        .find("read_fd=0")
        .expect("The JIT input read is present.");
    assert!(ambient_jit_check < home_initialization);
    assert!(home_initialization < listener_check && home_initialization < jit_read);
    assert!(!RUNNER_ENTRYPOINT.contains("/opt/runner/bin/Runner.Listener"));
    for required in [
        "HOME_ALLOWANCE_BYTES = 512 * 1024 * 1024",
        "HOME_LIMIT_BYTES = 2 * 1024 * 1024 * 1024",
        "nested != {work}",
        "names != {\"_work\"}",
        "source_required + HOME_ALLOWANCE_BYTES > HOME_LIMIT_BYTES",
        "os.rename(os.path.join(stage, name), target)",
        "home_manifest_mismatch",
        "listener_digest",
    ] {
        assert!(
            RUNNER_HOME_INIT.contains(required),
            "The Runner home volume contract is missing: {required}."
        );
    }
    for required in [
        "work mount fixture",
        "home_not_fresh",
        "home_capacity",
        "seed_manifest_mismatch",
        "seed_symlink_escape",
    ] {
        assert!(
            RUNNER_HOME_TEST.contains(required),
            "The Runner home volume test is missing: {required}."
        );
    }
}

fn assert_runtime_and_work_contract() {
    assert!(!RUNNER_ENTRYPOINT.contains("CARGO_BUILD_JOBS"));
    assert!(!RUNNER_ENTRYPOINT.contains("runner-job-env"));
    assert!(!RUNNER_DOCKERFILE.contains("CARGO_BUILD_JOBS"));
    assert!(!RUNNER_DOCKERFILE.contains("nproc"));
    assert!(!RUNNER_DOCKERFILE.contains("runner-job-env"));
    assert!(!RUNNER_README.contains("half of `nproc`"));
    assert!(RUNNER_README.contains("The image does not override Cargo's job count."));

    assert!(DIND_DOCKERFILE.contains("mkdir -p /var/lib/docker /run"));
    assert!(!DIND_DOCKERFILE.contains("/home/runner"));
    assert!(
        DIND_DOCKERFILE
            .contains("COPY --from=resource_probe resource-probe /velnor/resource-probe")
    );
    assert_eq!(
        RESOURCE_PROBE_BUILD
            .matches("--build-context \"resource_probe=$context\"")
            .count(),
        2
    );
    assert!(RUNNER_README.contains("/home/runner/_work"));
    assert!(RUNNER_README.contains("NoCopy"));
    assert!(RUNNER_README.contains("512 MiB allowance"));
    assert!(RUNNER_README.contains("workFolder: \"_work\""));
    assert!(DIND_README.contains("UID/GID `1000:1000`"));
    assert!(DIND_README.contains("bash images/resource-probe/build-runner-images.sh"));
}

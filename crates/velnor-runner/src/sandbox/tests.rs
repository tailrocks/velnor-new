// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};

use super::args::normalized;
use super::layout::{stage_dir_of, work_dir_of};
use super::parent::chain;
use super::trusted::Header;
use super::{Layout, MountOp, Profile, RootLink, SandboxArgs, plan};

const HOME_ROOT: &str = "/home";

mod accounting;
mod invocation;
mod network;

const TRUSTED_PULL_REQUEST: &str = include_str!("../../tests/fixtures/trusted/pull_request.json");

fn layout() -> Layout {
    Layout {
        root: PathBuf::from("/run/velnor-sandbox/7"),
        cgroup: PathBuf::from("/sys/fs/cgroup/velnor/job-7"),
        netns: PathBuf::from("/run/netns/velnor-job-7"),
        uid: 1000,
        gid: 1001,
        home: PathBuf::from("/home/runner"),
        work_dir: work_dir_of(7),
        stage_dir: stage_dir_of(7),
        store_read_only: vec![PathBuf::from("/var/lib/velnor/git/github.com/o/n/42.git")],
        root_links: vec![
            RootLink {
                name: "bin".to_owned(),
                target: PathBuf::from("usr/bin"),
            },
            RootLink {
                name: "lib".to_owned(),
                target: PathBuf::from("usr/lib"),
            },
        ],
    }
}

fn read_only(path: &str) -> MountOp {
    MountOp::BindReadOnly {
        source: PathBuf::from(path),
        target: PathBuf::from(path),
    }
}

fn tmpfs(target: &str, bytes: u64, mode: u32) -> MountOp {
    MountOp::Tmpfs {
        target: PathBuf::from(target),
        bytes,
        mode,
    }
}

fn link(name: &str, target: &str) -> MountOp {
    MountOp::Link {
        name: PathBuf::from(name),
        target: PathBuf::from(target),
    }
}

fn masked(directory: &str, own: &str) -> Vec<MountOp> {
    vec![
        tmpfs(directory, 1024 * 1024, 0o755),
        MountOp::BindReadWrite {
            source: PathBuf::from(own),
            target: PathBuf::from(own),
        },
        MountOp::SealReadOnly(PathBuf::from(directory)),
    ]
}

#[test]
fn job_plan_is_the_constructed_root() {
    let mut expected = vec![
        link("/bin", "usr/bin"),
        link("/lib", "usr/lib"),
        read_only("/usr"),
        read_only("/etc"),
        read_only("/opt/orbstack-guest/etc/resolv.conf"),
        read_only("/var/lib/velnor/bin"),
        read_only("/var/lib/velnor/tools"),
        read_only("/var/lib/velnor/crates"),
        read_only("/var/lib/velnor/git/github.com/o/n/42.git"),
        read_only("/srv/velnor/gen"),
    ];
    expected.extend([
        tmpfs("/srv/velnor/gen/work", 1024 * 1024, 0o755),
        MountOp::SealReadOnly(PathBuf::from("/srv/velnor/gen/work")),
        MountOp::BindReadWrite {
            source: PathBuf::from("/srv/velnor/gen/work/7"),
            target: PathBuf::from("/work"),
        },
    ]);
    expected.extend(masked("/srv/velnor/gen/stage", "/srv/velnor/gen/stage/7"));
    expected.extend([
        MountOp::BindReadWrite {
            source: PathBuf::from("/srv/velnor/gen/work/7/home"),
            target: PathBuf::from("/home/runner"),
        },
        MountOp::BindReadWrite {
            source: PathBuf::from("/srv/velnor/gen/work/7/tmp"),
            target: PathBuf::from("/tmp"),
        },
        tmpfs("/run", 16 * 1024 * 1024, 0o1777),
        tmpfs("/dev", 1024 * 1024, 0o755),
        MountOp::BindDevice(PathBuf::from("/dev/null")),
        MountOp::BindDevice(PathBuf::from("/dev/zero")),
        MountOp::BindDevice(PathBuf::from("/dev/full")),
        MountOp::BindDevice(PathBuf::from("/dev/random")),
        MountOp::BindDevice(PathBuf::from("/dev/urandom")),
        tmpfs("/dev/shm", 64 * 1024 * 1024, 0o1777),
        link("/dev/fd", "/proc/self/fd"),
        link("/dev/stdin", "/proc/self/fd/0"),
        link("/dev/stdout", "/proc/self/fd/1"),
        link("/dev/stderr", "/proc/self/fd/2"),
        MountOp::Proc,
    ]);
    assert_eq!(plan(Profile::Job, &layout()), expected);
}

#[test]
fn job_plan_writes_only_the_work_dir_home_tmp_and_stage() {
    let writable: Vec<PathBuf> = plan(Profile::Job, &layout())
        .into_iter()
        .filter_map(|op| match op {
            MountOp::BindReadWrite { source, target } => Some([source, target]),
            _ => None,
        })
        .flatten()
        .collect();
    let allowed = [
        "/srv/velnor/gen/work/7",
        "/work",
        "/srv/velnor/gen/stage/7",
        "/srv/velnor/gen/work/7/home",
        "/srv/velnor/gen/work/7/tmp",
        "/home/runner",
        "/tmp",
    ];
    for path in &writable {
        assert!(
            allowed.iter().any(|known| path == Path::new(known)),
            "{path:?}"
        );
    }
    assert!(
        !plan(Profile::Job, &layout()).iter().any(|op| matches!(
            op,
            MountOp::BindReadWrite { target, .. } if target.starts_with("/var/lib/velnor")
        )),
        "a store is writable"
    );
}

#[test]
fn job_plan_never_mounts_the_host_share_or_the_orbstack_runtime() {
    for op in plan(Profile::Job, &layout()) {
        let paths: Vec<&Path> = match &op {
            MountOp::BindReadOnly { source, target, .. }
            | MountOp::BindReadWrite { source, target } => vec![source, target],
            MountOp::Link { name, target } => vec![name, target],
            MountOp::BindDevice(path)
            | MountOp::SealReadOnly(path)
            | MountOp::Tmpfs { target: path, .. } => vec![path],
            MountOp::Proc => vec![],
        };
        for path in paths {
            for hidden in [
                "/var/lib/velnor-host",
                "/var/lib/velnor/locks",
                "/var/lib/velnor/jit",
                "/mnt",
                "/opt/orbstack-guest/run",
                "/opt/orbstack-guest/data",
                "/run/user",
            ] {
                assert!(!path.starts_with(hidden), "{op:?} reaches {hidden}");
            }
        }
    }
}

#[test]
fn sandbox_chain_enters_the_job_netns_then_unshares_under_a_death_signal() {
    let argv = chain(
        &layout(),
        Path::new("/var/lib/velnor/bin/velnor-runner"),
        &["/bin/true".to_owned()],
    )
    .unwrap_or_default();
    let head: Vec<&str> = argv.iter().take(16).map(String::as_str).collect();
    assert_eq!(
        head,
        [
            "--pdeathsig",
            "KILL",
            "--",
            "/usr/bin/nsenter",
            "--net=/run/netns/velnor-job-7",
            "--",
            "/usr/bin/unshare",
            "--mount",
            "--pid",
            "--ipc",
            "--uts",
            "--fork",
            "--kill-child",
            "--propagation",
            "private",
            "--",
        ]
    );
    assert_eq!(argv[16], "/var/lib/velnor/bin/velnor-runner");
    assert_eq!(argv[17], "sandbox-apply");
    assert_eq!(argv[18], "--layout");
    assert_eq!(argv[20], "--");
    assert_eq!(argv[21], "/bin/true");
    assert_eq!(
        serde_json::from_str::<Layout>(&argv[19]).ok().as_ref(),
        Some(&layout())
    );
}

type Adjust = Box<dyn FnOnce(&mut SandboxArgs)>;

fn arguments(adjust: impl FnOnce(&mut SandboxArgs)) -> SandboxArgs {
    let mut args = SandboxArgs {
        profile: Profile::Job,
        reservation: 7,
        uid: 1000,
        gid: 1001,
        home: PathBuf::from("/home/runner"),
        work_dir: work_dir_of(7),
        disk_limit_gib: 64,
        machine_floor_gib: 4,
        stdin_frames: false,
        max_jobs: 1,
        store_read_only: vec![PathBuf::from("/var/lib/velnor/git/github.com/o/n/42.git")],
        command: vec!["/bin/true".to_owned()],
    };
    adjust(&mut args);
    args
}

#[test]
fn arguments_are_refused_when_they_could_widen_the_sandbox() {
    assert!(arguments(|_| {}).validate().is_ok());
    let refused: [(&str, Adjust); 10] = [
        ("root uid", Box::new(|args| args.uid = 0)),
        ("root gid", Box::new(|args| args.gid = 0)),
        ("zero reservation", Box::new(|args| args.reservation = 0)),
        (
            "home outside /home",
            Box::new(|args| args.home = PathBuf::from("/etc")),
        ),
        (
            "home with a parent component",
            Box::new(|args| args.home = PathBuf::from(format!("{HOME_ROOT}/../etc"))),
        ),
        (
            "work dir of another job",
            Box::new(|args| args.work_dir = work_dir_of(8)),
        ),
        (
            "the whole cache store",
            Box::new(|args| args.store_read_only = vec![PathBuf::from("/var/lib/velnor/cache")]),
        ),
        (
            "a scope that climbs out of the cache store",
            Box::new(|args| {
                args.store_read_only = vec![PathBuf::from("/var/lib/velnor/cache/../tools/key")];
            }),
        ),
        (
            "the tools store as a scope",
            Box::new(|args| {
                args.store_read_only = vec![PathBuf::from("/var/lib/velnor/tools/key")];
            }),
        ),
        (
            "a relative command",
            Box::new(|args| args.command = vec!["true".to_owned()]),
        ),
    ];
    for (name, adjust) in refused {
        assert!(arguments(adjust).validate().is_err(), "{name}");
    }
}

#[test]
fn normalized_paths_have_no_dots_and_start_at_the_root() {
    assert!(normalized(Path::new("/home/runner")));
    assert!(!normalized(Path::new("home/user")));
    assert!(!normalized(Path::new(&format!("{HOME_ROOT}/./user"))));
    assert!(!normalized(Path::new(&format!("{HOME_ROOT}/../etc"))));
    assert!(!normalized(Path::new(&format!("{HOME_ROOT}//user"))));
    assert!(!normalized(Path::new("/home/runner/")));
}

#[cfg(target_os = "linux")]
#[test]
fn seccomp_programs_compile_for_vsock_user_namespaces_absent_calls_and_xattrs() {
    assert_eq!(
        super::seccomp::filters().map(|filters| filters.len()).ok(),
        Some(7)
    );
}

#[test]
fn trusted_facts_of_the_host_contract_parse() {
    let header = Header::parse(TRUSTED_PULL_REQUEST.as_bytes());
    assert_eq!(
        header.map(|header| header.describe()).ok().as_deref(),
        Some("repository tailrocks/velnor-lab(42) sha 0123456789ab event pull_request")
    );
    for broken in [
        TRUSTED_PULL_REQUEST.replace("tailrocks/velnor-lab", "velnor-lab"),
        TRUSTED_PULL_REQUEST.replace("0123456789abcdef0123456789abcdef01234567", "abc"),
        "[]".to_owned(),
    ] {
        assert!(Header::parse(broken.as_bytes()).is_err(), "{broken}");
    }
}

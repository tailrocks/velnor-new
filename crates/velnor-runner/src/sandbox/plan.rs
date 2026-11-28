// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};

use super::{GEN_ROOT, JOB_WORK, Layout, Profile, RESOLV_CONF, STORE_ROOT};

const STORE_READ_ONLY: [&str; 3] = ["bin", "tools", "crates"];
const DEVICES: [&str; 5] = ["null", "zero", "full", "random", "urandom"];
const DEVICE_LINKS: [(&str, &str); 4] = [
    ("fd", "/proc/self/fd"),
    ("stdin", "/proc/self/fd/0"),
    ("stdout", "/proc/self/fd/1"),
    ("stderr", "/proc/self/fd/2"),
];
const MIB: u64 = 1024 * 1024;
const DEV_BYTES: u64 = MIB;
const SHM_BYTES: u64 = 64 * MIB;
const RUN_BYTES: u64 = 16 * MIB;
const MASK_BYTES: u64 = MIB;
const STICKY: u32 = 0o1777;
const PUBLIC: u32 = 0o755;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MountOp {
    Link {
        name: PathBuf,
        target: PathBuf,
    },
    BindReadOnly {
        source: PathBuf,
        target: PathBuf,
    },
    BindReadWrite {
        source: PathBuf,
        target: PathBuf,
    },
    BindDevice(PathBuf),
    Tmpfs {
        target: PathBuf,
        bytes: u64,
        mode: u32,
    },
    SealReadOnly(PathBuf),
    Proc,
}

fn read_only(path: impl AsRef<Path>) -> MountOp {
    MountOp::BindReadOnly {
        source: path.as_ref().to_path_buf(),
        target: path.as_ref().to_path_buf(),
    }
}

fn writable(source: impl AsRef<Path>, target: impl AsRef<Path>) -> MountOp {
    MountOp::BindReadWrite {
        source: source.as_ref().to_path_buf(),
        target: target.as_ref().to_path_buf(),
    }
}

fn tmpfs(target: impl AsRef<Path>, bytes: u64, mode: u32) -> MountOp {
    MountOp::Tmpfs {
        target: target.as_ref().to_path_buf(),
        bytes,
        mode,
    }
}

fn masked(directory: PathBuf, own: &Path) -> [MountOp; 3] {
    [
        tmpfs(&directory, MASK_BYTES, PUBLIC),
        writable(own, own),
        MountOp::SealReadOnly(directory),
    ]
}

fn devices() -> Vec<MountOp> {
    let mut ops = vec![tmpfs("/dev", DEV_BYTES, PUBLIC)];
    ops.extend(
        DEVICES
            .iter()
            .map(|name| MountOp::BindDevice(Path::new("/dev").join(name))),
    );
    ops.push(tmpfs("/dev/shm", SHM_BYTES, STICKY));
    ops.extend(DEVICE_LINKS.iter().map(|(name, target)| MountOp::Link {
        name: Path::new("/dev").join(name),
        target: PathBuf::from(target),
    }));
    ops
}

#[must_use]
pub fn plan(profile: Profile, layout: &Layout) -> Vec<MountOp> {
    match profile {
        Profile::Job => job(layout),
    }
}

fn job(layout: &Layout) -> Vec<MountOp> {
    let mut ops: Vec<MountOp> = layout
        .root_links
        .iter()
        .map(|link| MountOp::Link {
            name: Path::new("/").join(&link.name),
            target: link.target.clone(),
        })
        .collect();
    ops.push(read_only("/usr"));
    ops.push(read_only("/etc"));
    ops.push(read_only(RESOLV_CONF));
    let store = Path::new(STORE_ROOT);
    ops.extend(
        STORE_READ_ONLY
            .iter()
            .map(|name| read_only(store.join(name))),
    );
    ops.extend(layout.store_read_only.iter().map(read_only));
    let generations = Path::new(GEN_ROOT);
    ops.push(read_only(generations));
    ops.extend([
        tmpfs(generations.join("work"), MASK_BYTES, PUBLIC),
        MountOp::SealReadOnly(generations.join("work")),
        writable(&layout.work_dir, JOB_WORK),
    ]);
    ops.extend(masked(generations.join("stage"), &layout.stage_dir));
    ops.push(writable(layout.work_dir.join("home"), &layout.home));
    ops.push(writable(layout.work_dir.join("tmp"), "/tmp"));
    ops.push(tmpfs("/run", RUN_BYTES, STICKY));
    ops.extend(devices());
    ops.push(MountOp::Proc);
    ops
}

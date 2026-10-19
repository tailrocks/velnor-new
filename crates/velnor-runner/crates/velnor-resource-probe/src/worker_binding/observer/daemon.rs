use std::{fs, os::fd::AsFd, path::Path};

use rustix::fs::{
    AtFlags, FileType, Mode, OFlags, StatxFlags, fstat, fstatfs, fstatvfs, open, openat, statx,
};

use crate::{
    ProbeError,
    filesystem::DOCKER_ROOT,
    worker_binding::{DockerRootObjectIdentity, ProcessWitness, StorageAttachmentObservation},
};

use super::process::observe_current_process;

const PROC_ROOT: &str = "/proc";
const PROC_FILE_LIMIT: usize = 256;
const MAX_PROC_ENTRIES: usize = 65_536;
const MAX_PATH_COMPONENTS: usize = 64;

pub(super) fn observe_selected_daemon(
    docker_root_path: &str,
    engine: &StorageAttachmentObservation,
) -> Result<(ProcessWitness, DockerRootObjectIdentity), ProbeError> {
    crate::worker_binding::observer::root_backing::validate_docker_root_path(docker_root_path)?;
    let root = open(
        DOCKER_ROOT,
        OFlags::PATH | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| ProbeError::Read("selected_docker_root", error.into()))?;
    let root_identity = object_identity(&root)?;
    if root_identity.mount_id != engine.statx_mount_id
        || root_identity.device_major != engine.statx_device_major
        || root_identity.device_minor != engine.statx_device_minor
        || root_identity.filesystem_type_magic != engine.filesystem_type_magic
        || root_identity.filesystem_id != engine.filesystem_id
    {
        return Err(ProbeError::Invalid("selected_docker_root_identity"));
    }

    let entries =
        fs::read_dir(PROC_ROOT).map_err(|error| ProbeError::Read("proc_inventory", error))?;
    let mut pids = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| ProbeError::Read("proc_inventory", error))?;
        let Some(pid) = entry.file_name().to_str().and_then(parse_pid) else {
            continue;
        };
        if pids.len() == MAX_PROC_ENTRIES {
            return Err(ProbeError::InputTooLarge("proc_inventory"));
        }
        pids.push(pid);
    }
    pids.sort_unstable();

    let mut candidates = Vec::new();
    for pid in pids {
        let Some(executable_before) = daemon_executable_identity(pid)? else {
            continue;
        };
        let witness = observe_current_process(pid)?;
        let candidate_root = process_docker_root(pid, docker_root_path)?;
        if daemon_executable_identity(pid)? != Some(executable_before) {
            return Err(ProbeError::Invalid("daemon_executable_changed"));
        }
        if let Some(candidate_root) = candidate_root {
            candidates.push((witness, executable_before, candidate_root));
        }
    }
    let (daemon, executable_identity) = unique_candidate(candidates, &root_identity)?;
    if object_identity(&root)? != root_identity
        || observe_current_process(daemon.host_pid)? != daemon
        || daemon_executable_identity(daemon.host_pid)? != Some(executable_identity)
        || process_docker_root(daemon.host_pid, docker_root_path)?.as_ref() != Some(&root_identity)
    {
        return Err(ProbeError::Invalid("selected_daemon_changed"));
    }
    Ok((daemon, root_identity))
}

fn unique_candidate(
    candidates: Vec<(ProcessWitness, ExecutableIdentity, DockerRootObjectIdentity)>,
    selected_root: &DockerRootObjectIdentity,
) -> Result<(ProcessWitness, ExecutableIdentity), ProbeError> {
    let mut matches = candidates
        .into_iter()
        .filter(|(_, _, root)| same_root_object(root, selected_root));
    let candidate = matches
        .next()
        .ok_or(ProbeError::Invalid("selected_daemon_missing"))?;
    if matches.next().is_some() {
        return Err(ProbeError::Invalid("selected_daemon_ambiguous"));
    }
    Ok((candidate.0, candidate.1))
}

fn same_root_object(
    candidate: &DockerRootObjectIdentity,
    selected: &DockerRootObjectIdentity,
) -> bool {
    candidate.device_major == selected.device_major
        && candidate.device_minor == selected.device_minor
        && candidate.inode == selected.inode
        && candidate.filesystem_type_magic == selected.filesystem_type_magic
        && candidate.filesystem_id == selected.filesystem_id
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ExecutableIdentity {
    device_major: u32,
    device_minor: u32,
    inode: u64,
}

fn daemon_executable_identity(pid: u32) -> Result<Option<ExecutableIdentity>, ProbeError> {
    let path = format!("{PROC_ROOT}/{pid}/comm");
    let bytes = match read_bounded(&path, PROC_FILE_LIMIT, "proc_comm") {
        Ok(bytes) => bytes,
        Err(ProbeError::Read(_, error)) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(None);
        }
        Err(error) => return Err(error),
    };
    if bytes.strip_suffix(b"\n") != Some(b"dockerd".as_slice()) {
        return Ok(None);
    }
    let executable = match fs::read_link(format!("{PROC_ROOT}/{pid}/exe")) {
        Ok(path) => path,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(ProbeError::Read("proc_executable", error)),
    };
    if executable.file_name().and_then(|name| name.to_str()) != Some("dockerd")
        || executable.to_string_lossy().contains(" (deleted)")
    {
        return Ok(None);
    }
    let descriptor = open(
        format!("{PROC_ROOT}/{pid}/exe"),
        OFlags::PATH | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| ProbeError::Read("daemon_executable", error.into()))?;
    let status = fstat(&descriptor)
        .map_err(|error| ProbeError::Read("daemon_executable_stat", error.into()))?;
    if FileType::from_raw_mode(status.st_mode) != FileType::RegularFile {
        return Err(ProbeError::Invalid("daemon_executable_type"));
    }
    let executable = statx(
        &descriptor,
        "",
        AtFlags::EMPTY_PATH | AtFlags::SYMLINK_NOFOLLOW,
        StatxFlags::BASIC_STATS,
    )
    .map_err(|error| ProbeError::Read("daemon_executable_statx", error.into()))?;
    if executable.stx_mask & StatxFlags::BASIC_STATS.bits() != StatxFlags::BASIC_STATS.bits()
        || executable.stx_ino == 0
    {
        return Err(ProbeError::Invalid("daemon_executable_statx_fields"));
    }
    Ok(Some(ExecutableIdentity {
        device_major: executable.stx_dev_major,
        device_minor: executable.stx_dev_minor,
        inode: executable.stx_ino,
    }))
}

fn process_docker_root(
    pid: u32,
    docker_root_path: &str,
) -> Result<Option<DockerRootObjectIdentity>, ProbeError> {
    let process_root = match open(
        format!("{PROC_ROOT}/{pid}/root"),
        OFlags::PATH | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
    ) {
        Ok(process_root) => process_root,
        Err(rustix::io::Errno::NOENT) => return Ok(None),
        Err(error) => return Err(ProbeError::Read("process_root", error.into())),
    };
    let components = docker_root_path
        .strip_prefix('/')
        .ok_or(ProbeError::Invalid("docker_root_path"))?
        .split('/')
        .collect::<Vec<_>>();
    if components.is_empty()
        || components.len() > MAX_PATH_COMPONENTS
        || components
            .iter()
            .any(|part| part.is_empty() || *part == "." || *part == "..")
    {
        return Err(ProbeError::Invalid("docker_root_path"));
    }
    let mut directory = process_root;
    for component in components {
        directory = match openat(
            &directory,
            component,
            OFlags::PATH | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        ) {
            Ok(directory) => directory,
            Err(rustix::io::Errno::NOENT) => return Ok(None),
            Err(error) => {
                return Err(ProbeError::Read("process_docker_root", error.into()));
            }
        };
    }
    object_identity(&directory).map(Some)
}

fn object_identity<Fd: AsFd>(descriptor: Fd) -> Result<DockerRootObjectIdentity, ProbeError> {
    let status =
        fstat(&descriptor).map_err(|error| ProbeError::Read("docker_root_stat", error.into()))?;
    if FileType::from_raw_mode(status.st_mode) != FileType::Directory {
        return Err(ProbeError::Invalid("docker_root_not_directory"));
    }
    let mount = statx(
        &descriptor,
        "",
        AtFlags::EMPTY_PATH | AtFlags::SYMLINK_NOFOLLOW,
        StatxFlags::BASIC_STATS | StatxFlags::MNT_ID,
    )
    .map_err(|error| ProbeError::Read("docker_root_statx", error.into()))?;
    let required = StatxFlags::BASIC_STATS | StatxFlags::MNT_ID;
    if mount.stx_mask & required.bits() != required.bits() || mount.stx_ino == 0 {
        return Err(ProbeError::Invalid("docker_root_statx_fields"));
    }
    let filesystem = fstatfs(&descriptor)
        .map_err(|error| ProbeError::Read("docker_root_statfs", error.into()))?;
    let capacity = fstatvfs(&descriptor)
        .map_err(|error| ProbeError::Read("docker_root_statvfs", error.into()))?;
    let filesystem_type_magic =
        u64::try_from(filesystem.f_type).map_err(|_| ProbeError::Invalid("docker_root_fs_type"))?;
    if filesystem_type_magic == 0 {
        return Err(ProbeError::Invalid("docker_root_filesystem_identity"));
    }
    Ok(DockerRootObjectIdentity {
        mount_id: mount.stx_mnt_id,
        device_major: mount.stx_dev_major,
        device_minor: mount.stx_dev_minor,
        inode: mount.stx_ino,
        filesystem_type_magic,
        filesystem_id: capacity.f_fsid,
    })
}

fn parse_pid(value: &str) -> Option<u32> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    value.parse().ok().filter(|pid| *pid > 0)
}

fn read_bounded(path: &str, limit: usize, name: &'static str) -> Result<Vec<u8>, ProbeError> {
    use std::io::Read;

    let file = fs::File::open(Path::new(path)).map_err(|error| ProbeError::Read(name, error))?;
    let mut bytes = Vec::with_capacity(limit.min(256));
    file.take(u64::try_from(limit + 1).map_err(|_| ProbeError::Overflow("input_limit"))?)
        .read_to_end(&mut bytes)
        .map_err(|error| ProbeError::Read(name, error))?;
    if bytes.len() > limit {
        return Err(ProbeError::InputTooLarge(name));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::{ExecutableIdentity, unique_candidate};
    use crate::worker_binding::{DockerRootObjectIdentity, ProcessWitness};

    fn identity(inode: u64) -> DockerRootObjectIdentity {
        DockerRootObjectIdentity {
            mount_id: 4,
            device_major: 8,
            device_minor: 1,
            inode,
            filesystem_type_magic: 0xef53,
            filesystem_id: 9,
        }
    }

    fn witness(pid: u32) -> ProcessWitness {
        ProcessWitness {
            host_pid: pid,
            start_ticks: 10,
            mount_namespace_id: 20,
            mount_namespace_device_major: 0,
            mount_namespace_device_minor: 5,
            namespace_pids: vec![pid, 1],
            cgroup_path: "/system.slice/docker.service".to_owned(),
        }
    }

    fn executable(inode: u64) -> ExecutableIdentity {
        ExecutableIdentity {
            device_major: 8,
            device_minor: 1,
            inode,
        }
    }

    #[test]
    fn selected_daemon_requires_one_exact_root_object_match() {
        let selected = identity(30);
        let mut candidate_root = selected.clone();
        candidate_root.mount_id = 99;
        let candidate = (witness(43), executable(50), candidate_root);
        let chosen =
            unique_candidate(vec![candidate.clone()], &selected).expect("Find one root candidate.");
        assert_eq!(chosen.0, candidate.0);
        assert_eq!(chosen.1, candidate.1);
        assert!(unique_candidate(vec![], &selected).is_err());
        assert!(
            unique_candidate(vec![(witness(44), executable(51), identity(31))], &selected).is_err()
        );
        assert!(
            unique_candidate(
                vec![
                    candidate.clone(),
                    (witness(44), executable(51), selected.clone())
                ],
                &selected,
            )
            .is_err()
        );
    }
}

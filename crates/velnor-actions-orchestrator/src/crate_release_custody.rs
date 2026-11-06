// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    io::IoSliceMut,
    mem::MaybeUninit,
    os::{
        fd::{AsFd, AsRawFd, OwnedFd},
        unix::{fs::MetadataExt, net::UnixStream},
    },
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use rustix::{
    net::{RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, ReturnFlags},
    time::{ClockId, clock_gettime},
};

static TAKEN: AtomicBool = AtomicBool::new(false);

pub(crate) struct PublisherRoot {
    directory: OwnedFd,
    path: PathBuf,
}

pub(crate) struct PublisherView {
    directory: OwnedFd,
    path: PathBuf,
    exposed: bool,
}

pub(crate) struct CustodiedChannel {
    pub(crate) root: PublisherRoot,
    pub(crate) view: PublisherView,
    pub(crate) socket: UnixStream,
}

impl PublisherRoot {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn descriptor(&self) -> &OwnedFd {
        &self.directory
    }

    pub(crate) fn revalidate(&self) -> Result<(), &'static str> {
        validate_directory(&self.directory, &self.path, false, 0o700)
    }
}

impl PublisherView {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn descriptor(&self) -> &OwnedFd {
        &self.directory
    }

    pub(crate) fn revalidate(&self) -> Result<(), &'static str> {
        validate_directory(
            &self.directory,
            &self.path,
            true,
            if self.exposed { 0o711 } else { 0o700 },
        )
    }

    pub(crate) fn expose(&mut self) -> Result<(), &'static str> {
        self.expose_inner(|| Ok(()))
    }

    #[cfg(test)]
    pub(crate) fn expose_with_hook(
        &mut self,
        after_chmod: impl FnOnce() -> Result<(), &'static str>,
    ) -> Result<(), &'static str> {
        self.expose_inner(after_chmod)
    }

    fn expose_inner(
        &mut self,
        after_chmod: impl FnOnce() -> Result<(), &'static str>,
    ) -> Result<(), &'static str> {
        if self.exposed {
            return Err("publisher_custody_invalid");
        }
        self.revalidate()?;
        rustix::fs::fchmod(&self.directory, rustix::fs::Mode::from_raw_mode(0o711))
            .map_err(|_| "publisher_custody_invalid")?;
        self.exposed = true;
        after_chmod()?;
        self.revalidate()
    }
}

impl CustodiedChannel {
    pub(crate) fn take(deadline: u64) -> Result<Self, &'static str> {
        TAKEN
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| "publisher_custody_consumed")?;
        let socket = UnixStream::from(
            rustix::io::dup(std::io::stdin().as_fd()).map_err(|_| "publisher_custody_invalid")?,
        );
        let private = receive_directory(&socket, b'R', deadline, false)?;
        let view = receive_directory(&socket, b'V', deadline, true)?;
        let private_meta = std::fs::File::from(
            rustix::io::dup(&private.0).map_err(|_| "publisher_custody_invalid")?,
        )
        .metadata()
        .map_err(|_| "publisher_custody_invalid")?;
        let view_meta =
            std::fs::File::from(rustix::io::dup(&view.0).map_err(|_| "publisher_custody_invalid")?)
                .metadata()
                .map_err(|_| "publisher_custody_invalid")?;
        if private_meta.dev() == view_meta.dev() && private_meta.ino() == view_meta.ino() {
            return Err("publisher_custody_invalid");
        }
        check_deadline(deadline)?;
        Ok(Self {
            root: PublisherRoot {
                directory: private.0,
                path: private.1,
            },
            view: PublisherView {
                directory: view.0,
                path: view.1,
                exposed: false,
            },
            socket,
        })
    }
}

fn check_deadline(deadline: u64) -> Result<(), &'static str> {
    let time = clock_gettime(ClockId::Boottime);
    let now = u64::try_from(time.tv_sec)
        .ok()
        .and_then(|seconds| seconds.checked_mul(1_000_000_000))
        .and_then(|nanos| {
            u64::try_from(time.tv_nsec)
                .ok()
                .and_then(|part| nanos.checked_add(part))
        })
        .ok_or("publisher_custody_deadline")?;
    if now >= deadline {
        Err("publisher_custody_deadline")
    } else {
        Ok(())
    }
}

fn receive_directory(
    socket: &UnixStream,
    marker: u8,
    deadline: u64,
    view: bool,
) -> Result<(OwnedFd, PathBuf), &'static str> {
    loop {
        check_deadline(deadline)?;
        let mut byte = [0_u8; 1];
        let mut storage = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(1))];
        let mut ancillary = RecvAncillaryBuffer::new(&mut storage);
        let mut slices = [IoSliceMut::new(&mut byte)];
        match rustix::net::recvmsg(
            socket,
            &mut slices,
            &mut ancillary,
            RecvFlags::DONTWAIT | RecvFlags::CMSG_CLOEXEC,
        ) {
            Ok(message) => {
                let mut descriptors = Vec::new();
                let mut valid = true;
                for item in ancillary.drain() {
                    match item {
                        RecvAncillaryMessage::ScmRights(fds) => descriptors.extend(fds),
                        _ => valid = false,
                    }
                }
                if !valid
                    || message.bytes != 1
                    || byte[0] != marker
                    || message
                        .flags
                        .intersects(ReturnFlags::CTRUNC | ReturnFlags::TRUNC)
                    || descriptors.len() != 1
                {
                    return Err("publisher_custody_invalid");
                }
                let fd = descriptors.pop().ok_or("publisher_custody_invalid")?;
                let path = std::fs::read_link(format!("/proc/self/fd/{}", fd.as_raw_fd()))
                    .map_err(|_| "publisher_custody_invalid")?;
                validate_directory(&fd, &path, view, 0o700)?;
                check_deadline(deadline)?;
                return Ok((fd, path));
            }
            Err(rustix::io::Errno::AGAIN | rustix::io::Errno::INTR) => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(_) => return Err("publisher_custody_invalid"),
        }
    }
}

fn validate_directory(
    fd: &OwnedFd,
    path: &Path,
    view: bool,
    mode: u32,
) -> Result<(), &'static str> {
    let invalid = || "publisher_custody_invalid";
    let run = std::fs::symlink_metadata("/run").map_err(|_| invalid())?;
    let descriptor = std::fs::File::from(rustix::io::dup(fd).map_err(|_| invalid())?);
    let metadata = descriptor.metadata().map_err(|_| invalid())?;
    let current = std::fs::symlink_metadata(path).map_err(|_| invalid())?;
    let name = path
        .file_name()
        .and_then(|part| part.to_str())
        .ok_or_else(invalid)?;
    let suffix = name.strip_prefix("velnor-publisher-").ok_or_else(invalid)?;
    if !run.is_dir()
        || run.file_type().is_symlink()
        || run.uid() != 0
        || run.mode() & 0o022 != 0
        || path.parent() != Some(Path::new("/run"))
        || suffix.is_empty()
        || !suffix
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
        || suffix.starts_with("view-") != view
        || !metadata.is_dir()
        || metadata.uid() != 0
        || metadata.mode() & 0o7777 != mode
        || !current.is_dir()
        || current.file_type().is_symlink()
        || current.uid() != 0
        || current.mode() & 0o7777 != mode
        || current.dev() != metadata.dev()
        || current.ino() != metadata.ino()
    {
        return Err(invalid());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::PublisherView;
    use std::{fs, os::unix::fs::PermissionsExt as _};

    #[test]
    #[ignore = "requires isolated root-owned /run"]
    fn exposed_view_rejects_reuse_permission_change_and_path_replacement()
    -> Result<(), Box<dyn std::error::Error>> {
        let view = tempfile::Builder::new()
            .prefix("velnor-publisher-view-")
            .tempdir_in("/run")?;
        fs::set_permissions(view.path(), fs::Permissions::from_mode(0o700))?;
        let directory = fs::File::open(view.path())?.into();
        let mut held = PublisherView {
            directory,
            path: view.path().to_owned(),
            exposed: false,
        };
        held.revalidate()?;
        held.expose()?;
        held.revalidate()?;
        assert!(held.expose().is_err(), "view was exposed twice");
        fs::set_permissions(view.path(), fs::Permissions::from_mode(0o700))?;
        assert!(
            held.revalidate().is_err(),
            "changed view permissions were accepted"
        );
        fs::set_permissions(view.path(), fs::Permissions::from_mode(0o711))?;
        held.revalidate()?;
        let moved = view.path().with_extension("displaced");
        fs::rename(view.path(), &moved)?;
        fs::create_dir(view.path())?;
        fs::set_permissions(view.path(), fs::Permissions::from_mode(0o711))?;
        assert!(held.revalidate().is_err(), "replacement view was accepted");
        fs::remove_dir(view.path())?;
        fs::rename(moved, view.path())?;
        held.revalidate()?;
        Ok(())
    }

    #[test]
    #[ignore = "requires isolated root-owned /run"]
    fn exposure_rejects_replacement_before_revalidation() -> Result<(), Box<dyn std::error::Error>>
    {
        let view = tempfile::Builder::new()
            .prefix("velnor-publisher-view-")
            .tempdir_in("/run")?;
        fs::set_permissions(view.path(), fs::Permissions::from_mode(0o700))?;
        let mut held = PublisherView {
            directory: fs::File::open(view.path())?.into(),
            path: view.path().to_owned(),
            exposed: false,
        };
        let moved = view.path().with_extension("displaced");
        let result = held.expose_inner(|| {
            fs::rename(view.path(), &moved).map_err(|_| "fixture failure")?;
            fs::create_dir(view.path()).map_err(|_| "fixture failure")?;
            fs::set_permissions(view.path(), fs::Permissions::from_mode(0o711))
                .map_err(|_| "fixture failure")?;
            Ok(())
        });
        fs::remove_dir(view.path())?;
        fs::rename(moved, view.path())?;
        assert!(result.is_err(), "replacement during exposure was accepted");
        Ok(())
    }
}

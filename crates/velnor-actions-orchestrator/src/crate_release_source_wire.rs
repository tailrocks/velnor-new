// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    io::{IoSlice, IoSliceMut, Write},
    mem::MaybeUninit,
    os::{fd::AsFd, unix::net::UnixStream},
    time::Duration,
};

use rustix::{
    fs::{MemfdFlags, SealFlags},
    net::{
        RecvAncillaryBuffer, RecvFlags, ReturnFlags, SendAncillaryBuffer, SendAncillaryMessage,
        SendFlags,
    },
    time::{ClockId, clock_gettime},
};

const MAX_CONTEXT: usize = 32 * 1024 * 1024;
const MAX_RESPONSE: usize = 64 * 1024 * 1024;

#[derive(Clone, Copy)]
pub(crate) enum SourceOperation {
    Init,
    Metadata,
    Bind,
    CredentialPreflight,
    Package { index: usize, verify: bool },
    Semver { index: usize },
    Upload { index: usize },
    Close,
}

pub(crate) struct SourceWire {
    socket: UnixStream,
    deadline: u64,
    sequence: u64,
    poisoned: bool,
}

impl SourceWire {
    pub(crate) fn new(socket: UnixStream, deadline: u64) -> Result<Self, &'static str> {
        socket
            .set_nonblocking(true)
            .map_err(|_| "source_channel_invalid")?;
        let wire = Self {
            socket,
            deadline,
            sequence: 0,
            poisoned: false,
        };
        wire.remaining()?;
        Ok(wire)
    }

    pub(crate) fn poison(&mut self) {
        self.poisoned = true;
        let _closed = self.socket.shutdown(std::net::Shutdown::Both);
    }

    pub(crate) fn request(
        &mut self,
        operation: SourceOperation,
        context: Option<&[u8]>,
    ) -> Result<Vec<u8>, &'static str> {
        if self.poisoned {
            return Err("source_channel_closed");
        }
        let result = self.request_inner(operation, context);
        if result.is_err() {
            self.poison();
        }
        result
    }

    fn request_inner(
        &mut self,
        operation: SourceOperation,
        context: Option<&[u8]>,
    ) -> Result<Vec<u8>, &'static str> {
        let sequence = self
            .sequence
            .checked_add(1)
            .ok_or("source_channel_invalid")?;
        let (name, index, needs_context, limit) = match operation {
            SourceOperation::Init => ("init", None, true, 0),
            SourceOperation::Metadata => ("metadata", None, false, 16 * 1024 * 1024),
            SourceOperation::Bind => ("bind", None, true, 0),
            SourceOperation::CredentialPreflight => ("preflight", None, false, 0),
            SourceOperation::Package { index, verify } => (
                if verify { "package-verify" } else { "package" },
                Some(index),
                false,
                MAX_RESPONSE,
            ),
            SourceOperation::Semver { index } => ("semver", Some(index), true, 0),
            SourceOperation::Upload { index } => ("upload", Some(index), true, 0),
            SourceOperation::Close => ("close", None, false, 0),
        };
        if needs_context != context.is_some() {
            return Err("source_channel_invalid");
        }
        let mut request = serde_json::json!({"seq": sequence, "op": name});
        if let Some(index) = index {
            request["index"] = serde_json::json!(index);
        }
        let bytes = serde_json::to_vec(&request).map_err(|_| "source_channel_invalid")?;
        let length = u32::try_from(bytes.len()).map_err(|_| "source_channel_invalid")?;
        self.write_all(&length.to_be_bytes())?;
        self.write_all(&bytes)?;
        if let Some(context) = context {
            self.send_context(context)?;
        }
        let mut header = [0_u8; 12];
        self.read_exact(&mut header)?;
        let response_seq = u64::from_be_bytes(
            header[..8]
                .try_into()
                .map_err(|_| "source_channel_invalid")?,
        );
        let response_len = u32::from_be_bytes(
            header[8..]
                .try_into()
                .map_err(|_| "source_channel_invalid")?,
        ) as usize;
        if response_seq != sequence || response_len > limit || response_len >= MAX_RESPONSE {
            return Err("source_channel_invalid");
        }
        let mut response = vec![0_u8; response_len];
        self.read_exact(&mut response)?;
        self.remaining()?;
        self.sequence = sequence;
        Ok(response)
    }

    pub(crate) fn remaining(&self) -> Result<(), &'static str> {
        self.remaining_duration().map(|_| ())
    }

    pub(crate) fn remaining_duration(&self) -> Result<Duration, &'static str> {
        if self.poisoned {
            return Err("source_channel_closed");
        }
        let current = clock_gettime(ClockId::Boottime);
        let seconds = u64::try_from(current.tv_sec).map_err(|_| "source_channel_deadline")?;
        let nanos = u64::try_from(current.tv_nsec).map_err(|_| "source_channel_deadline")?;
        let now = seconds
            .checked_mul(1_000_000_000)
            .and_then(|value| value.checked_add(nanos))
            .ok_or("source_channel_deadline")?;
        self.deadline
            .checked_sub(now)
            .filter(|remaining| *remaining > 0)
            .map(Duration::from_nanos)
            .ok_or("source_channel_deadline")
    }

    fn write_all(&mut self, mut data: &[u8]) -> Result<(), &'static str> {
        while !data.is_empty() {
            self.remaining()?;
            match rustix::net::send(
                &self.socket,
                data,
                SendFlags::DONTWAIT | SendFlags::NOSIGNAL,
            ) {
                Ok(0) => return Err("source_channel_closed"),
                Ok(written) => data = &data[written..],
                Err(rustix::io::Errno::AGAIN | rustix::io::Errno::INTR) => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(_) => return Err("source_channel_closed"),
            }
        }
        Ok(())
    }

    fn read_exact(&self, mut data: &mut [u8]) -> Result<(), &'static str> {
        while !data.is_empty() {
            self.remaining()?;
            let mut control_bytes = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(1))];
            let mut control = RecvAncillaryBuffer::new(&mut control_bytes);
            let mut buffers = [IoSliceMut::new(data)];
            match rustix::net::recvmsg(
                &self.socket,
                &mut buffers,
                &mut control,
                RecvFlags::DONTWAIT | RecvFlags::CMSG_CLOEXEC,
            ) {
                Ok(result) => {
                    let has_control = control.drain().next().is_some();
                    if has_control
                        || result
                            .flags
                            .intersects(ReturnFlags::CTRUNC | ReturnFlags::TRUNC)
                        || result.bytes == 0
                    {
                        return Err("source_channel_invalid");
                    }
                    data = &mut data[result.bytes..];
                }
                Err(rustix::io::Errno::AGAIN | rustix::io::Errno::INTR) => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(_) => return Err("source_channel_closed"),
            }
        }
        Ok(())
    }

    fn send_context(&self, bytes: &[u8]) -> Result<(), &'static str> {
        if bytes.is_empty() || bytes.len() > MAX_CONTEXT {
            return Err("source_channel_invalid");
        }
        let descriptor = rustix::fs::memfd_create(
            "velnor-source-context",
            MemfdFlags::CLOEXEC | MemfdFlags::ALLOW_SEALING,
        )
        .map_err(|_| "source_channel_invalid")?;
        let mut file = std::fs::File::from(descriptor);
        file.write_all(bytes)
            .map_err(|_| "source_channel_invalid")?;
        rustix::fs::fcntl_add_seals(
            &file,
            SealFlags::WRITE | SealFlags::GROW | SealFlags::SHRINK | SealFlags::SEAL,
        )
        .map_err(|_| "source_channel_invalid")?;
        let descriptors = [file.as_fd()];
        let mut control_bytes = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(1))];
        let mut control = SendAncillaryBuffer::new(&mut control_bytes);
        if !control.push(SendAncillaryMessage::ScmRights(&descriptors)) {
            return Err("source_channel_invalid");
        }
        self.remaining()?;
        let sent = rustix::net::sendmsg(
            &self.socket,
            &[IoSlice::new(b"C")],
            &mut control,
            SendFlags::DONTWAIT | SendFlags::NOSIGNAL,
        )
        .map_err(|_| "source_channel_closed")?;
        if sent != 1 {
            return Err("source_channel_closed");
        }
        Ok(())
    }
}

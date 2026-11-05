# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import ctypes
import errno
import os
import re
import socket
import stat
import struct
import time


class ManagerError(OSError):
    pass


class Manager:
    def __init__(self, helper, runtime_seconds, transport=None, root_bootstrap=False):
        if (not os.path.isabs(helper) or type(runtime_seconds) is not int
                or not 1 <= runtime_seconds <= 86400 or type(root_bootstrap) is not bool
                or (root_bootstrap and os.geteuid() != 0)):
            raise ManagerError("invalid manager request")
        if transport is None and (os.geteuid() != 0 or "DBUS_SYSTEM_BUS_ADDRESS" in os.environ):
            raise ManagerError("untrusted system bus environment")
        self.helper = helper
        self.runtime_seconds = runtime_seconds
        self.root_bootstrap = root_bootstrap
        self.library = ctypes.CDLL("libsystemd.so.0")
        self.bus = ctypes.c_void_p()
        self.events = []
        self.owner = None
        self.unit_path = None
        self.dropin_parent = None
        self.dropin_name = None
        self.slot = ctypes.c_void_p()
        self._bind()
        try:
            if transport is None:
                self._connect_system()
            else:
                transport(self)
            if self._check(self.library.sd_bus_is_bus_client(self.bus)) != 1:
                raise ManagerError("brokered system bus required")
        except Exception:
            self.close()
            raise

    def _connect_system(self):
        flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC
        directory = os.open("/", flags)
        try:
            for component in ("run", "dbus"):
                metadata = os.fstat(directory)
                if metadata.st_uid != 0 or metadata.st_mode & 0o022:
                    raise ManagerError("untrusted system bus parent")
                child = os.open(component, flags, dir_fd=directory)
                os.close(directory)
                directory = child
            metadata = os.fstat(directory)
            if metadata.st_uid != 0 or metadata.st_mode & 0o022:
                raise ManagerError("untrusted system bus parent")
            before = os.stat("system_bus_socket", dir_fd=directory, follow_symlinks=False)
            if before.st_uid != 0 or not stat.S_ISSOCK(before.st_mode):
                raise ManagerError("untrusted system bus socket")
            self._check(self.library.sd_bus_open_system(ctypes.byref(self.bus)))
            after = os.stat("system_bus_socket", dir_fd=directory, follow_symlinks=False)
            if (before.st_dev, before.st_ino) != (after.st_dev, after.st_ino):
                raise ManagerError("system bus socket changed")
            descriptor = self._check(self.library.sd_bus_get_fd(self.bus))
            connection = socket.fromfd(descriptor, socket.AF_UNIX, socket.SOCK_STREAM)
            try:
                pid, uid, _gid = struct.unpack("3i", connection.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12))
            finally:
                connection.close()
            if uid != 0 or pid <= 0:
                raise ManagerError("untrusted system bus peer")
            peer_namespace = os.stat("/proc/" + str(pid) + "/ns/pid")
            init_namespace = os.stat("/proc/1/ns/pid")
            if (peer_namespace.st_dev, peer_namespace.st_ino) != (init_namespace.st_dev, init_namespace.st_ino):
                raise ManagerError("untrusted system bus namespace")
            executable = os.stat("/proc/" + str(pid) + "/exe")
            trusted = []
            for path in ("/usr/lib/systemd/systemd", "/usr/bin/dbus-daemon", "/usr/bin/dbus-broker",
                         "/usr/bin/dbus-broker-launch"):
                try:
                    candidate = os.stat(path)
                except FileNotFoundError:
                    continue
                if candidate.st_uid == 0 and stat.S_ISREG(candidate.st_mode) and not candidate.st_mode & 0o022:
                    trusted.append((candidate.st_dev, candidate.st_ino))
            if (executable.st_dev, executable.st_ino) not in trusted:
                raise ManagerError("untrusted system bus peer")
        finally:
            os.close(directory)

    def _bind(self):
        p = ctypes.c_void_p
        s = ctypes.c_char_p
        i = ctypes.c_int
        c = ctypes.c_char
        u = ctypes.c_uint64
        funcs = {
            "sd_bus_open_system": ([ctypes.POINTER(p)], i),
            "sd_bus_is_bus_client": ([p], i),
            "sd_bus_get_fd": ([p], i),
            "sd_bus_set_method_call_timeout": ([p, u], i),
            "sd_bus_unref": ([p], p),
            "sd_bus_slot_unref": ([p], p),
            "sd_bus_message_unref": ([p], p),
            "sd_bus_message_new_method_call": ([p, ctypes.POINTER(p), s, s, s, s], i),
            "sd_bus_message_append_basic": ([p, c, p], i),
            "sd_bus_message_open_container": ([p, c, s], i),
            "sd_bus_message_close_container": ([p], i),
            "sd_bus_message_enter_container": ([p, c, s], i),
            "sd_bus_message_read_basic": ([p, c, p], i),
            "sd_bus_message_read_array": ([p, c, ctypes.POINTER(p), ctypes.POINTER(ctypes.c_size_t)], i),
            "sd_bus_call": ([p, p, u, p, ctypes.POINTER(p)], i),
            "sd_bus_process": ([p, p], i),
            "sd_bus_wait": ([p, u], i),
            "sd_bus_add_match": ([p, ctypes.POINTER(p), s, p, p], i),
        }
        for name, (args, result) in funcs.items():
            function = getattr(self.library, name)
            function.argtypes = args
            function.restype = result

    @staticmethod
    def _check(result):
        if result < 0:
            raise ManagerError("system bus operation failed: " + str(result))
        return result

    def _message(self, destination, path, interface, member):
        message = ctypes.c_void_p()
        self._check(self.library.sd_bus_message_new_method_call(
            self.bus, ctypes.byref(message), destination.encode(), path.encode(), interface.encode(), member.encode()))
        return message

    def _append(self, message, kind, value):
        if kind in "sog":
            data = ctypes.c_char_p(value.encode())
            pointer = ctypes.cast(data, ctypes.c_void_p)
        else:
            data = {"b": ctypes.c_int, "h": ctypes.c_int, "t": ctypes.c_uint64}[kind](value)
            pointer = ctypes.cast(ctypes.byref(data), ctypes.c_void_p)
        self._check(self.library.sd_bus_message_append_basic(message, kind.encode(), pointer))

    def _open(self, message, kind, signature):
        self._check(self.library.sd_bus_message_open_container(message, kind.encode(), signature.encode()))

    def _close(self, message):
        self._check(self.library.sd_bus_message_close_container(message))

    def _enter(self, message, kind, signature):
        if self._check(self.library.sd_bus_message_enter_container(message, kind.encode(), signature.encode())) != 1:
            raise ManagerError("invalid bus response")

    def _read(self, message, kind):
        if kind in "sog":
            value = ctypes.c_char_p()
            self._check(self.library.sd_bus_message_read_basic(message, kind.encode(), ctypes.byref(value)))
            if value.value is None:
                raise ManagerError("invalid bus response")
            return value.value.decode("utf-8")
        value = {"u": ctypes.c_uint32, "i": ctypes.c_int32, "t": ctypes.c_uint64}[kind]()
        if self._check(self.library.sd_bus_message_read_basic(message, kind.encode(), ctypes.byref(value))) != 1:
            raise ManagerError("invalid bus response")
        return value.value

    def _call(self, message, deadline):
        reply = ctypes.c_void_p()
        remaining = deadline - time.clock_gettime_ns(time.CLOCK_BOOTTIME)
        if remaining <= 0:
            raise ManagerError("system bus deadline")
        self._check(self.library.sd_bus_call(self.bus, message, max(1, int(remaining // 1_000)), None,
                                             ctypes.byref(reply)))
        return reply

    def _method(self, destination, path, interface, member, args, deadline, result=None):
        message = self._message(destination, path, interface, member)
        reply = None
        try:
            for kind, value in args:
                self._append(message, kind, value)
            reply = self._call(message, deadline)
            return self._read(reply, result) if result else None
        finally:
            if reply:
                self.library.sd_bus_message_unref(reply)
            self.library.sd_bus_message_unref(message)

    def _owner(self, deadline):
        return self._method("org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus",
                            "GetNameOwner", [("s", "org.freedesktop.systemd1")], deadline, "s")

    def subscribe(self, deadline):
        remaining = deadline - time.clock_gettime_ns(time.CLOCK_BOOTTIME)
        if remaining <= 0:
            raise ManagerError("system bus deadline")
        self._check(self.library.sd_bus_set_method_call_timeout(self.bus, max(1, int(remaining // 1_000))))
        self.owner = self._owner(deadline)
        callback_type = ctypes.CFUNCTYPE(ctypes.c_int, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p)

        def callback(message, _userdata, _error):
            try:
                self._read(message, "u")
                path = self._read(message, "o")
                unit = self._read(message, "s")
                result = self._read(message, "s")
                if len(self.events) >= 4096:
                    return -1
                self.events.append((path, unit, result))
                return 0
            except (ManagerError, UnicodeError):
                return -1

        self.callback = callback_type(callback)
        match = ("type='signal',sender='org.freedesktop.systemd1',path='/org/freedesktop/systemd1',"
                 "interface='org.freedesktop.systemd1.Manager',member='JobRemoved'")
        self._check(self.library.sd_bus_add_match(self.bus, ctypes.byref(self.slot), match.encode(), self.callback, None))
        self._method(self.owner, "/org/freedesktop/systemd1",
                     "org.freedesktop.systemd1.Manager", "Subscribe", [], deadline)
        if self._owner(deadline) != self.owner:
            raise ManagerError("manager owner changed")
        return self.owner

    def _property(self, message, name, kind, value):
        self._open(message, "r", "sv")
        self._append(message, "s", name)
        self._open(message, "v", kind)
        self._append(message, kind, value)
        self._close(message)
        self._close(message)

    def _prepare_unit(self, unit):
        if os.geteuid() != 0 or not re.fullmatch(r"velnor-release-[0-9a-f]{32}\.service", unit):
            raise ManagerError("invalid unit staging")
        flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC
        directory = os.open("/", flags)
        try:
            for component in ("run", "systemd", "system"):
                metadata = os.fstat(directory)
                if metadata.st_uid != 0 or metadata.st_mode & 0o022:
                    raise ManagerError("untrusted unit staging parent")
                child = os.open(component, flags, dir_fd=directory)
                os.close(directory)
                directory = child
            metadata = os.fstat(directory)
            if metadata.st_uid != 0 or metadata.st_mode & 0o022:
                raise ManagerError("untrusted unit staging parent")
            name = unit + ".d"
            os.mkdir(name, 0o700, dir_fd=directory)
            self.dropin_parent = directory
            self.dropin_name = name
            directory = None
            child = os.open(name, flags, dir_fd=self.dropin_parent)
            try:
                fd = os.open("deadline.conf", os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC,
                             0o600, dir_fd=child)
                try:
                    data = ("[Unit]\nJobTimeoutSec=" + str(self.runtime_seconds) + "s\n").encode("ascii")
                    while data:
                        written = os.write(fd, data)
                        if not written:
                            raise ManagerError("unit deadline staging failed")
                        data = data[written:]
                    os.fsync(fd)
                finally:
                    os.close(fd)
            finally:
                os.close(child)
        finally:
            if directory is not None:
                os.close(directory)

    def release(self, unit, deadline):
        if self.dropin_name != unit + ".d":
            raise ManagerError("invalid unit release")
        self._method(self.owner, "/org/freedesktop/systemd1", "org.freedesktop.systemd1.Manager", "UnrefUnit",
                     [("s", unit)], deadline)
        flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC
        directory = os.open(self.dropin_name, flags, dir_fd=self.dropin_parent)
        try:
            os.unlink("deadline.conf", dir_fd=directory)
        finally:
            os.close(directory)
        os.rmdir(self.dropin_name, dir_fd=self.dropin_parent)
        os.close(self.dropin_parent)
        self.dropin_parent = None
        self.dropin_name = None

    def start(self, unit, reader, deadline):
        self._prepare_unit(unit)
        message = self._message(self.owner, "/org/freedesktop/systemd1", "org.freedesktop.systemd1.Manager",
                                "StartTransientUnit")
        reply = None
        try:
            self._append(message, "s", unit)
            self._append(message, "s", "fail")
            self._open(message, "a", "(sv)")
            for name, kind, value in (
                ("AddRef", "b", 1), ("DynamicUser", "b", int(not self.root_bootstrap)), ("NoNewPrivileges", "b", 1), ("Delegate", "b", 0),
                ("CapabilityBoundingSet", "t", 0), ("AmbientCapabilities", "t", 0), ("LimitCORE", "t", 0),
                ("TimeoutStartUSec", "t", 5_000_000), ("TimeoutStopUSec", "t", 20_000_000),
                ("RuntimeMaxUSec", "t", self.runtime_seconds * 1_000_000),
                ("KillMode", "s", "control-group"), ("StandardOutput", "s", "null"),
                ("StandardError", "s", "null"), ("StandardInputFileDescriptor", "h", reader),
            ):
                self._property(message, name, kind, value)
            self._open(message, "r", "sv")
            self._append(message, "s", "ExecStart")
            self._open(message, "v", "a(sasb)")
            self._open(message, "a", "(sasb)")
            self._open(message, "r", "sasb")
            self._append(message, "s", "/usr/bin/python3")
            self._open(message, "a", "s")
            for arg in ("/usr/bin/python3", "-I", "-S", self.helper):
                self._append(message, "s", arg)
            self._close(message)
            self._append(message, "b", 0)
            for _ in range(4):
                self._close(message)
            self._close(message)
            self._open(message, "a", "(sa(sv))")
            self._close(message)
            reply = self._call(message, deadline)
            job = self._read(reply, "o")
        finally:
            if reply:
                self.library.sd_bus_message_unref(reply)
            self.library.sd_bus_message_unref(message)
        self.unit_path = self._method(self.owner, "/org/freedesktop/systemd1", "org.freedesktop.systemd1.Manager",
                                      "GetUnit", [("s", unit)], deadline, "o")
        if self._get("org.freedesktop.systemd1.Unit", "JobTimeoutUSec", "t", deadline) != self.runtime_seconds * 1_000_000:
            raise ManagerError("unit deadline unverified")
        return job

    def event(self, deadline):
        while not self.events:
            remaining = deadline - time.clock_gettime_ns(time.CLOCK_BOOTTIME)
            if remaining <= 0:
                return None
            if self._check(self.library.sd_bus_process(self.bus, None)) == 0:
                self._check(self.library.sd_bus_wait(self.bus, max(1, int(min(remaining, 100_000_000) // 1_000))))
        return self.events.pop(0)

    def stop(self, unit, deadline):
        return self._method(self.owner, "/org/freedesktop/systemd1", "org.freedesktop.systemd1.Manager",
                            "StopUnit", [("s", unit), ("s", "replace")], deadline, "o")

    def _get(self, interface, name, kind, deadline):
        message = self._message(self.owner, self.unit_path, "org.freedesktop.DBus.Properties", "Get")
        reply = None
        try:
            self._append(message, "s", interface)
            self._append(message, "s", name)
            reply = self._call(message, deadline)
            self._enter(reply, "v", kind)
            if kind == "ay":
                pointer = ctypes.c_void_p()
                size = ctypes.c_size_t()
                self._check(self.library.sd_bus_message_read_array(reply, b"y", ctypes.byref(pointer), ctypes.byref(size)))
                return ctypes.string_at(pointer, size.value).hex()
            if kind == "(uo)":
                self._enter(reply, "r", "uo")
                self._read(reply, "u")
                value = self._read(reply, "o")
                return None if value == "/" else value
            return self._read(reply, kind)
        finally:
            if reply:
                self.library.sd_bus_message_unref(reply)
            self.library.sd_bus_message_unref(message)

    def state(self, unit, deadline):
        if not re.fullmatch(r"velnor-release-[0-9a-f]{32}\.service", unit):
            raise ManagerError("invalid unit")
        unit_interface = "org.freedesktop.systemd1.Unit"
        service = "org.freedesktop.systemd1.Service"
        path = "/sys/fs/cgroup/system.slice/" + unit + "/cgroup.events"
        identity = None
        for _ in range(8):
            if time.clock_gettime_ns(time.CLOCK_BOOTTIME) >= deadline:
                raise ManagerError("unit state deadline")
            owner = self._owner(deadline)
            value = {
                "owner": owner,
                "invocation": self._get(unit_interface, "InvocationID", "ay", deadline),
                "control_group": self._get(service, "ControlGroup", "s", deadline),
                "active": self._get(unit_interface, "ActiveState", "s", deadline),
                "job": self._get(unit_interface, "Job", "(uo)", deadline),
                "result": self._get(service, "Result", "s", deadline),
                "exec_code": self._get(service, "ExecMainCode", "i", deadline),
                "exec_status": self._get(service, "ExecMainStatus", "i", deadline),
            }
            current = (owner, value["invocation"])
            if identity is not None and identity != current:
                raise ManagerError("unit identity changed")
            try:
                with open(path, encoding="ascii") as stream:
                    data = stream.read(4097)
            except FileNotFoundError:
                value["populated"] = False
            except OSError as error:
                if error.errno != errno.ENODEV:
                    raise
                if self._owner(deadline) != owner:
                    raise ManagerError("manager owner changed") from None
                identity = current
                continue
            else:
                matches = re.findall(r"^populated ([01])$", data, re.MULTILINE)
                if len(data) > 4096 or len(matches) != 1:
                    raise ManagerError("invalid cgroup state")
                value["populated"] = matches[0] == "1"
            if self._owner(deadline) != owner:
                raise ManagerError("manager owner changed")
            return value
        raise ManagerError("unit state remained unavailable")

    def close(self):
        if self.dropin_parent is not None:
            os.close(self.dropin_parent)
            self.dropin_parent = None
        if self.slot:
            self.library.sd_bus_slot_unref(self.slot)
            self.slot = None
        if self.bus:
            self.library.sd_bus_unref(self.bus)
            self.bus = None

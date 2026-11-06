# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import os
import re
import secrets
import select
import struct
import time


class SupervisorError(Exception):
    pass


def wait_job(manager, unit, job, deadline):
    while time.clock_gettime_ns(time.CLOCK_BOOTTIME) < deadline:
        event = manager.event(deadline)
        if event is None:
            continue
        path, name, result = event
        if path != job:
            continue
        if name != unit or result != "done":
            raise SupervisorError("unit job failed")
        return
    raise SupervisorError("unit job deadline")


def state(manager, owner, unit, deadline):
    value = manager.state(unit, deadline)
    if value["owner"] != owner:
        raise SupervisorError("manager owner changed")
    return value


def write_frame(writer, frame, deadline):
    if not frame or len(frame) > 1024 * 1024 + 20:
        raise SupervisorError("invalid frame")
    os.set_blocking(writer, False)
    view = memoryview(frame)
    while view:
        remaining = deadline - time.clock_gettime_ns(time.CLOCK_BOOTTIME)
        if remaining <= 0:
            raise SupervisorError("frame deadline")
        if not select.select([], [writer], [], min(remaining / 1_000_000_000, 0.1))[1]:
            continue
        written = os.write(writer, view)
        if written == 0:
            raise SupervisorError("frame delivery failed")
        view = view[written:]


def supervise(manager, frame, deadline, cleanup_reserve=25_000_000_000, output_fd=None):
    if (type(deadline) is not int or not 0 < deadline < 2**64 or type(frame) is not bytes
            or len(frame) < 20 or type(cleanup_reserve) is not int or cleanup_reserve <= 0):
        raise SupervisorError("invalid frame deadline")
    if deadline - time.clock_gettime_ns(time.CLOCK_BOOTTIME) <= cleanup_reserve + 250_000_000:
        raise SupervisorError("deadline too close")
    work_deadline = deadline - cleanup_reserve
    frame_deadline, frame_namespace, frame_size = struct.unpack(">QQI", frame[:20])
    namespace = os.stat("/proc/self/ns/time")
    manager_namespace = os.stat("/proc/1/ns/time")
    if (frame_deadline != deadline or frame_namespace != namespace.st_ino
            or (namespace.st_dev, namespace.st_ino) != (manager_namespace.st_dev, manager_namespace.st_ino)
            or not 0 < frame_size <= 1024 * 1024 or frame_size != len(frame) - 20):
        raise SupervisorError("invalid frame deadline")
    unit = "velnor-release-" + secrets.token_hex(16) + ".service"
    owner = manager.subscribe(work_deadline)
    if not isinstance(owner, str) or not owner.startswith(":"):
        raise SupervisorError("manager identity unavailable")
    reader, writer = os.pipe()
    job = None
    released = False
    try:
        try:
            if output_fd is None:
                start_job = manager.start(unit, reader, work_deadline)
            else:
                start_job = manager.start(unit, reader, work_deadline, output_fd=output_fd)
        finally:
            os.close(reader)
        if not isinstance(start_job, str) or not re.fullmatch(r"/org/freedesktop/systemd1/job/[0-9]+", start_job):
            raise SupervisorError("start job unavailable")
        job = start_job
        wait_job(manager, unit, job, work_deadline)
        current = state(manager, owner, unit, work_deadline)
        invocation = current["invocation"]
        if (not re.fullmatch(r"[0-9a-f]{32}", invocation)
                or current["control_group"] != "/system.slice/" + unit
                or current["active"] != "active" or current["job"] is not None or not current["populated"]):
            raise SupervisorError("unit identity unavailable")
        write_frame(writer, frame, work_deadline)
        os.close(writer)
        writer = None
        while time.clock_gettime_ns(time.CLOCK_BOOTTIME) < work_deadline:
            current = state(manager, owner, unit, work_deadline)
            if current["invocation"] != invocation:
                raise SupervisorError("unit identity changed")
            if current["active"] == "failed":
                raise SupervisorError("unit failed")
            if current["active"] == "inactive":
                if (current["job"] is not None or current["populated"] or current["result"] != "success"
                        or current["exec_code"] != 1 or current["exec_status"] != 0):
                    raise SupervisorError("unit completion unverified")
                if time.clock_gettime_ns(time.CLOCK_BOOTTIME) >= work_deadline:
                    raise SupervisorError("unit completion exceeded deadline")
                manager.release(unit, deadline)
                released = True
                if time.clock_gettime_ns(time.CLOCK_BOOTTIME) >= work_deadline:
                    raise SupervisorError("unit completion exceeded deadline")
                return
            time.sleep(min(0.01, max(0, work_deadline - time.clock_gettime_ns(time.CLOCK_BOOTTIME)) / 1_000_000_000))
        raise SupervisorError("unit deadline")
    except Exception as error:
        if writer is not None:
            os.close(writer)
            writer = None
        if released:
            raise SupervisorError("unit completion exceeded deadline") from None
        if job is None:
            raise SupervisorError("unit cleanup uncertain") from None
        try:
            stop = manager.stop(unit, deadline)
            if not isinstance(stop, str) or not re.fullmatch(r"/org/freedesktop/systemd1/job/[0-9]+", stop):
                raise SupervisorError("stop job unavailable")
            wait_job(manager, unit, stop, deadline)
            current = state(manager, owner, unit, deadline)
            if current["active"] not in ("inactive", "failed") or current["job"] is not None or current["populated"]:
                raise SupervisorError("unit cleanup uncertain")
            manager.release(unit, deadline)
            if time.clock_gettime_ns(time.CLOCK_BOOTTIME) >= deadline:
                raise SupervisorError("unit cleanup exceeded deadline")
        except Exception:
            raise SupervisorError("unit cleanup uncertain") from None
        raise SupervisorError("unit failed after verified cleanup") from error
    finally:
        if writer is not None:
            os.close(writer)

# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import array
import json
import os
import select
import socket
import struct
import time


class SourceChannelError(Exception):
    pass


def ready(control, deadline, write):
    while True:
        remaining = deadline - time.clock_gettime_ns(time.CLOCK_BOOTTIME)
        if remaining <= 0:
            raise SourceChannelError('source channel deadline')
        read_set = [] if write else [control]
        write_set = [control] if write else []
        readable, writable, _ = select.select(read_set, write_set, [], min(.1, remaining / 1_000_000_000))
        if readable or writable:
            return


def read_exact(control, size, deadline):
    result = bytearray()
    while len(result) < size:
        ready(control, deadline, False)
        try:
            data, ancillary, flags, _ = control.recvmsg(size - len(result), socket.CMSG_SPACE(array.array('i').itemsize),
                                                        socket.MSG_CMSG_CLOEXEC | socket.MSG_DONTWAIT)
        except BlockingIOError:
            continue
        for level, kind, payload in ancillary:
            if level == socket.SOL_SOCKET and kind == socket.SCM_RIGHTS:
                values = array.array('i')
                width = values.itemsize
                values.frombytes(payload[:len(payload) - len(payload) % width])
                for descriptor in values:
                    os.close(descriptor)
        if ancillary or flags & (socket.MSG_CTRUNC | socket.MSG_TRUNC):
            raise SourceChannelError('unexpected source descriptor')
        if not data:
            raise SourceChannelError('source channel closed')
        result.extend(data)
    return bytes(result)


def write_all(control, data, deadline):
    view = memoryview(data)
    while view:
        ready(control, deadline, True)
        try:
            size = control.send(view, socket.MSG_DONTWAIT | socket.MSG_NOSIGNAL)
        except BlockingIOError:
            continue
        if size <= 0:
            raise SourceChannelError('source channel closed')
        view = view[size:]


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise SourceChannelError('duplicate request field')
        result[key] = value
    return result


def request(control, expected, deadline):
    size = struct.unpack('>I', read_exact(control, 4, deadline))[0]
    if not 0 < size <= 1024:
        raise SourceChannelError('invalid source request')
    try:
        value = json.loads(read_exact(control, size, deadline), object_pairs_hook=unique_object)
    except (ValueError, UnicodeError):
        raise SourceChannelError('invalid source request') from None
    if (not isinstance(value, dict) or type(value.get('seq')) is not int or value['seq'] != expected
            or not isinstance(value.get('op'), str)):
        raise SourceChannelError('invalid source request')
    if value['op'] in ('init', 'metadata', 'bind', 'preflight', 'close'):
        if set(value) != {'seq', 'op'}:
            raise SourceChannelError('invalid source request')
    elif value['op'] in ('package', 'package-verify', 'semver', 'upload'):
        if set(value) != {'seq', 'op', 'index'} or type(value['index']) is not int or value['index'] < 0:
            raise SourceChannelError('invalid source request')
    else:
        raise SourceChannelError('invalid source request')
    return value


def receive_context(control, reader, deadline):
    width = array.array('i').itemsize
    while True:
        ready(control, deadline, False)
        try:
            data, ancillary, flags, _ = control.recvmsg(1, socket.CMSG_SPACE(width),
                                                        socket.MSG_CMSG_CLOEXEC | socket.MSG_DONTWAIT)
            break
        except BlockingIOError:
            continue
    descriptors = []
    malformed = False
    try:
        for level, kind, payload in ancillary:
            if level != socket.SOL_SOCKET or kind != socket.SCM_RIGHTS:
                malformed = True
                continue
            if len(payload) % width:
                malformed = True
            values = array.array('i')
            values.frombytes(payload[:len(payload) - len(payload) % width])
            descriptors.extend(values)
        if (malformed or data != b'C' or flags & (socket.MSG_CTRUNC | socket.MSG_TRUNC)
                or len(descriptors) != 1 or time.clock_gettime_ns(time.CLOCK_BOOTTIME) >= deadline):
            raise SourceChannelError('invalid source context')
        return reader(descriptors[0])
    finally:
        for descriptor in descriptors:
            os.close(descriptor)


def response(control, sequence, data, deadline):
    if not isinstance(data, bytes) or len(data) >= 64 * 1024 * 1024:
        raise SourceChannelError('invalid source response')
    write_all(control, struct.pack('>QI', sequence, len(data)), deadline)
    write_all(control, data, deadline)
    if time.clock_gettime_ns(time.CLOCK_BOOTTIME) >= deadline:
        raise SourceChannelError('source channel deadline')


def serve(control, manager, locator, lifecycle, executor, uploader, broker, secrets, root, deadline, view_identity):
    sequence = 1
    first = request(control, sequence, deadline)
    if first['op'] != 'init':
        raise SourceChannelError('source context required')
    initial = receive_context(control, executor.read_context, deadline)
    session = executor.SourceSession(manager, locator, lifecycle, root, initial, deadline, view_identity)
    response(control, sequence, b'', deadline)
    while True:
        sequence += 1
        if sequence > 2**64 - 1:
            raise SourceChannelError('source sequence exhausted')
        value = request(control, sequence, deadline)
        op = value['op']
        if op == 'metadata':
            data = session.metadata()
        elif op == 'bind':
            session.bind(receive_context(control, executor.read_context, deadline))
            data = b''
        elif op == 'package' or op == 'package-verify':
            data = session.package(value['index'], op == 'package-verify')
        elif op == 'semver':
            data = session.semver(value['index'], receive_context(control, executor.read_semver_context, deadline))
        elif op == 'preflight':
            if session.bound is None:
                raise SourceChannelError('source context required')
            uploader.preflight(session, secrets(session.bound.authentication))
            data = b''
        elif op == 'upload':
            details = receive_context(control, lambda fd: uploader.read_upload_context(fd, executor), deadline)
            uploader.upload(session, executor, broker, details, value['index'], secrets(session.bound.authentication))
            data = b''
        elif op == 'close':
            session.verify_view()
            if manager.dropin_name is not None or locator.dropin_name is not None:
                raise SourceChannelError('source cleanup uncertain')
            manager.close()
            locator.close()
            response(control, sequence, b'', deadline)
            return
        else:
            raise SourceChannelError('invalid source transition')
        response(control, sequence, data, deadline)

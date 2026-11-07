# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import ctypes
import errno
import os
import platform
import socket
import sys


class Filter(ctypes.Structure):
    _fields_ = [("code", ctypes.c_ushort), ("jt", ctypes.c_ubyte), ("jf", ctypes.c_ubyte), ("k", ctypes.c_uint)]


class Program(ctypes.Structure):
    _fields_ = [("length", ctypes.c_ushort), ("filter", ctypes.POINTER(Filter))]


calls = {
    "x86_64": (41, 42, 43, 49, 50, 288),
    "aarch64": (198, 203, 202, 200, 201, 242),
}
blocked = calls[platform.machine()]
instructions = [Filter(0x20, 0, 0, 0)]
for call in blocked:
    instructions += [Filter(0x15, 0, 1, call), Filter(0x06, 0, 0, 0x00050000 | errno.EPERM)]
instructions.append(Filter(0x06, 0, 0, 0x7FFF0000))
filters = (Filter * len(instructions))(*instructions)
program = Program(len(filters), filters)
libc = ctypes.CDLL(None, use_errno=True)
if libc.prctl(38, 1, 0, 0, 0) != 0 or libc.prctl(22, 2, ctypes.byref(program)) != 0:
    raise OSError(ctypes.get_errno(), "cannot deny network")
try:
    socket.socket(socket.AF_INET, socket.SOCK_STREAM)
except OSError as error:
    if error.errno != errno.EPERM:
        raise
else:
    raise RuntimeError("network sandbox allowed socket creation")
os.execv(sys.argv[1], sys.argv[1:])

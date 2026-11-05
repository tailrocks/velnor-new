# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import json
import os
import re
import struct
import sys
import time


LIMIT = 1024 * 1024


def read_exact(size):
    result = bytearray()
    while len(result) < size:
        part = os.read(0, size - len(result))
        if not part:
            raise ValueError("incomplete frame")
        result.extend(part)
    return result


def unique_object(pairs):
    value = {}
    for key, item in pairs:
        if key in value:
            raise ValueError("duplicate field")
        value[key] = item
    return value


def decode_frame():
    deadline, namespace, size = struct.unpack(">QQI", read_exact(20))
    if (deadline <= time.clock_gettime_ns(time.CLOCK_BOOTTIME)
            or namespace != os.stat("/proc/self/ns/time").st_ino):
        raise ValueError("invalid deadline")
    if not 0 < size <= LIMIT:
        raise ValueError("invalid frame size")
    frame = json.loads(read_exact(size), object_pairs_hook=unique_object)
    if not isinstance(frame, dict) or set(frame) != {"program", "args", "cwd", "env", "stdin"}:
        raise ValueError("invalid frame")
    program, args, cwd, env, stdin = (frame[key] for key in ("program", "args", "cwd", "env", "stdin"))
    if not isinstance(program, str) or not os.path.isabs(program):
        raise ValueError("invalid program")
    if not isinstance(cwd, str) or not os.path.isabs(cwd):
        raise ValueError("invalid working directory")
    if not isinstance(args, list) or not all(isinstance(arg, str) for arg in args):
        raise ValueError("invalid arguments")
    if not isinstance(env, dict) or not all(
        re.fullmatch(r"[A-Z_][A-Z0-9_]*", key) and isinstance(value, str)
        for key, value in env.items()
    ):
        raise ValueError("invalid environment")
    if any(key.startswith(("LD_", "DYLD_", "PYTHON")) or key in ("BASH_ENV", "ENV", "GLIBC_TUNABLES") for key in env):
        raise ValueError("invalid environment")
    if any("\0" in value for value in [program, cwd, *args, *env.values()]):
        raise ValueError("invalid string")
    if not isinstance(stdin, str):
        raise ValueError("invalid input")
    stdin = stdin.encode("utf-8")
    if len(stdin) > 4096:
        raise ValueError("invalid input")
    return program, args, cwd, env, stdin, deadline


def main():
    try:
        program, args, cwd, env, stdin, deadline = decode_frame()
        os.closerange(3, os.sysconf("SC_OPEN_MAX"))
        reader, writer = os.pipe()
        os.set_blocking(writer, False)
        view = memoryview(stdin)
        while view:
            view = view[os.write(writer, view):]
        os.close(writer)
        os.dup2(reader, 0)
        os.close(reader)
        os.chdir(cwd)
        if deadline <= time.clock_gettime_ns(time.CLOCK_BOOTTIME):
            raise ValueError("deadline expired")
        os.execve(program, [program, *args], env)
    except (OSError, ValueError, UnicodeError, json.JSONDecodeError):
        raise SystemExit("supervisor frame rejected") from None


if __name__ == "__main__":
    main()

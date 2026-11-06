# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import os
import re
import stat


def main():
    root = os.path.dirname(os.path.abspath(__file__))
    if (os.geteuid() != 0 or os.path.dirname(root) != '/run'
            or not re.fullmatch(r'velnor-publisher-[a-zA-Z0-9_-]+', os.path.basename(root))):
        raise ValueError('invalid supervisor claim')
    flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC
    parent = os.open('/run', flags)
    try:
        parent_info = os.fstat(parent)
        if parent_info.st_uid != 0 or parent_info.st_mode & 0o022:
            raise ValueError('invalid supervisor claim')
        directory = os.open(os.path.basename(root), flags, dir_fd=parent)
    finally:
        os.close(parent)
    try:
        info = os.fstat(directory)
        if info.st_uid != 0 or stat.S_IMODE(info.st_mode) != 0o700:
            raise ValueError('invalid supervisor claim')
        descriptor = os.open('supervisor.claim', os.O_WRONLY | os.O_CREAT | os.O_EXCL
                             | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600, dir_fd=directory)
        try:
            os.fchmod(descriptor, 0o600)
            info = os.fstat(descriptor)
            if (not stat.S_ISREG(info.st_mode) or info.st_uid != 0 or info.st_nlink != 1
                    or stat.S_IMODE(info.st_mode) != 0o600):
                raise ValueError('invalid supervisor claim')
            os.fsync(descriptor)
        finally:
            os.close(descriptor)
        os.fsync(directory)
    finally:
        os.close(directory)


if __name__ == '__main__':
    try:
        main()
    except (OSError, ValueError):
        raise SystemExit('supervisor claim rejected') from None

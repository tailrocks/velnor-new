# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import os
import pathlib
import runpy
import sys

script, home, scratch, filename, receipt = sys.argv[1:]
real_open = os.open

def substitute(path, flags, mode=0o777, *, dir_fd=None):
    if path == filename and dir_fd is not None:
        os.unlink(path, dir_fd=dir_fd)
        os.mkfifo(path, 0o600, dir_fd=dir_fd)
        pathlib.Path(receipt).write_text("substituted")
    return real_open(path, flags, mode, dir_fd=dir_fd)

os.open = substitute
sys.argv = [script, "inspect", home, scratch, "0", "0"]
runpy.run_path(script, run_name="__main__")

# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import os
import stat


FILES = {
    "src": ("release_generator_bootstrap.sh", "release_manager_bus.py", "release_manager_lifecycle.py",
            "release_publish_entry.py", "release_supervisor_exec.py", "release_coordinator_exec.py",
            "release_launcher_watch.py", "release_supervisor_claim.py", "release_retained_parent.py",
            "release_source_channel.py", "release_bootstrap_watch.py", "release_watch_admission.py"),
    "tests": ("release_qualification_source.py", "release_watch_fixture.py",
              "release_manager_scenario_controller_test.py", "release_manager_proxy_test.py", "release_manager_qualification.py",
              "release_manager_scenario_controller.py", "release_manager_systemd_test.py",
              "release_publish_entry_offline_test.py", "release_publish_entry_systemd_test.py",
              "release_publish_entry_test.py", "release_coordinator_exec_test.py",
              "release_retained_entry_systemd_test.py"),
}


def stage(source, destination):
    source_fd = os.open(source, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC)
    try:
        for subdirectory, names in FILES.items():
            target = destination / subdirectory
            target.mkdir(mode=0o755)
            parent = os.open(subdirectory, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC,
                             dir_fd=source_fd)
            try:
                for name in names:
                    descriptor = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC | os.O_NONBLOCK, dir_fd=parent)
                    try:
                        metadata = os.fstat(descriptor)
                        if not stat.S_ISREG(metadata.st_mode) or metadata.st_size > 1024 * 1024:
                            raise SystemExit("invalid qualification source")
                        with os.fdopen(descriptor, "rb", closefd=False) as input_file:
                            data = input_file.read(1024 * 1024 + 1)
                        after = os.fstat(descriptor)
                        if (len(data) != metadata.st_size or metadata.st_size != after.st_size
                                or metadata.st_mtime_ns != after.st_mtime_ns
                                or metadata.st_ctime_ns != after.st_ctime_ns):
                            raise SystemExit("qualification source changed")
                    finally:
                        os.close(descriptor)
                    (target / name).write_bytes(data)
                    (target / name).chmod(0o444)
            finally:
                os.close(parent)
    finally:
        os.close(source_fd)

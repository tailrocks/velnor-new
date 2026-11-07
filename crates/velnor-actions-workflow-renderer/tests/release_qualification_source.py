# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import os
import stat
import sys
from pathlib import Path


FILES = {
    "src": ("release_generator_bootstrap.sh", "release_manager_bus.py", "release_manager_lifecycle.py",
            "release_publish_entry.py", "release_supervisor_exec.py", "release_coordinator_exec.py",
            "release_launcher_watch.py", "release_supervisor_claim.py", "release_retained_parent.py",
            "release_source_executor.py", "release_source_channel.py", "release_upload_operation.py",
            "release_forge_broker.py", "release_bootstrap_watch.py", "release_watch_admission.py", "release_manager_policy_host.py",
            "release_manager_policy_files.py", "release_manager_policy.py", "release_export_contract.py",
            "release_rendered_entry.py"),
    "tests": ("release_qualification_source.py", "release_watch_fixture.py",
              "release_manager_policy_systemd_test.py",
              "release_manager_scenario_controller_test.py", "release_manager_proxy_test.py", "release_manager_qualification.py",
              "release_manager_scenario_controller.py", "release_manager_systemd_test.py",
              "release_publish_entry_offline_test.py", "release_publish_entry_systemd_test.py",
              "release_publish_entry_test.py", "release_coordinator_exec_test.py",
              "release_retained_entry_systemd_test.py"),
}


def stage(source, destination, rendered_entry=None):
    source_fd = os.open(source, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC)
    try:
        for subdirectory, names in FILES.items():
            target = destination / subdirectory
            target.mkdir(mode=0o755)
            parent = os.open(subdirectory, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC,
                             dir_fd=source_fd)
            try:
                for name in names:
                    file = rendered_entry if name == "release_rendered_entry.py" and rendered_entry is not None else name
                    descriptor = os.open(file, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC | os.O_NONBLOCK, dir_fd=parent)
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


if __name__ == "__main__":
    if len(sys.argv) != 5 or sys.argv[1] != "--stage":
        raise SystemExit("invalid qualification staging request")
    stage(Path(sys.argv[2]), Path(sys.argv[3]), Path(sys.argv[4]))

# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import importlib.util
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import sys
import tempfile
import unittest


root = Path(__file__).resolve().parent
scenarios = {
    "escaped-descendant": ("release_manager_systemd_test", "RealManagerTest", "test_detached_term_resistant_descendant_is_gone_before_completion"),
    "malformed-cgroup-injected": ("release_manager_systemd_test", "RealManagerTest", "test_malformed_kernel_cgroup_state_cannot_verify_cleanup"),
    "delivery": ("release_manager_systemd_test", "RealManagerTest", "test_real_manager_delivers_and_removes_owned_unit_dropin"),
    "queued-timeout": ("release_manager_systemd_test", "RealManagerTest", "test_real_queued_start_times_out_then_stop_converges"),
    "parent-death-queued": ("release_manager_systemd_test", "RealManagerTest", "test_parent_death_while_accepted_start_is_queued"),
    "parent-death-delivered": ("release_manager_systemd_test", "RealManagerTest", "test_manager_runtime_reaps_delivered_child_after_supervisor_death"),
    "staging-symlink-refusal": ("release_manager_systemd_test", "RealManagerTest", "test_root_owned_staging_rejects_preexisting_symlink"),
    "fake-broker-refusal": ("release_manager_proxy_test", "RealReplyLossTest", "test_production_constructor_rejects_inherited_fake_broker"),
    "reply-loss-late-start": ("release_manager_proxy_test", "RealReplyLossTest", "test_accepted_start_runs_late_after_reply_loss_without_input"),
    "root-context-modes": ("release_publish_entry_test", "PublisherEntryTest", "test_context_accepts_only_matching_release_modes"),
    "root-context-rejections": ("release_publish_entry_test", "PublisherEntryTest", "test_context_rejects_unapproved_identity_and_credentials"),
    "root-context-ref": ("release_publish_entry_test", "PublisherEntryTest", "test_context_rejects_invalid_branch_even_when_workflow_ref_matches"),
    "root-binary-custody": ("release_publish_entry_test", "PublisherEntryTest", "test_executable_requires_exact_root_owned_regular_bytes"),
    "root-unit-clean": ("release_publish_entry_systemd_test", "PublisherEntrySystemdTest", "test_root_bootstrap_finishes_only_after_unit_quiesces"),
    "root-unit-descendant": ("release_publish_entry_systemd_test", "PublisherEntrySystemdTest", "test_root_bootstrap_kills_detached_descendant_before_return"),
    "root-unit-parent-death": ("release_publish_entry_systemd_test", "PublisherEntrySystemdTest", "test_root_bootstrap_manager_reaps_child_after_parent_death"),
    "root-main-credential": ("release_publish_entry_offline_test", "PublisherEntryOfflineTest", "test_main_passes_credential_to_real_bootstrap"),
}


capability_v1 = {
    ("escaped-descendant", "release_manager_systemd_test", "RealManagerTest", "test_detached_term_resistant_descendant_is_gone_before_completion"),
    ("malformed-cgroup-injected", "release_manager_systemd_test", "RealManagerTest", "test_malformed_kernel_cgroup_state_cannot_verify_cleanup"),
    ("delivery", "release_manager_systemd_test", "RealManagerTest", "test_real_manager_delivers_and_removes_owned_unit_dropin"),
    ("queued-timeout", "release_manager_systemd_test", "RealManagerTest", "test_real_queued_start_times_out_then_stop_converges"),
    ("parent-death-queued", "release_manager_systemd_test", "RealManagerTest", "test_parent_death_while_accepted_start_is_queued"),
    ("parent-death-delivered", "release_manager_systemd_test", "RealManagerTest", "test_manager_runtime_reaps_delivered_child_after_supervisor_death"),
    ("staging-symlink-refusal", "release_manager_systemd_test", "RealManagerTest", "test_root_owned_staging_rejects_preexisting_symlink"),
    ("fake-broker-refusal", "release_manager_proxy_test", "RealReplyLossTest", "test_production_constructor_rejects_inherited_fake_broker"),
    ("reply-loss-late-start", "release_manager_proxy_test", "RealReplyLossTest", "test_accepted_start_runs_late_after_reply_loss_without_input"),
    ("root-context-modes", "release_publish_entry_test", "PublisherEntryTest", "test_context_accepts_only_matching_release_modes"),
    ("root-context-rejections", "release_publish_entry_test", "PublisherEntryTest", "test_context_rejects_unapproved_identity_and_credentials"),
    ("root-context-ref", "release_publish_entry_test", "PublisherEntryTest", "test_context_rejects_invalid_branch_even_when_workflow_ref_matches"),
    ("root-binary-custody", "release_publish_entry_test", "PublisherEntryTest", "test_executable_requires_exact_root_owned_regular_bytes"),
    ("root-unit-clean", "release_publish_entry_systemd_test", "PublisherEntrySystemdTest", "test_root_bootstrap_finishes_only_after_unit_quiesces"),
    ("root-unit-descendant", "release_publish_entry_systemd_test", "PublisherEntrySystemdTest", "test_root_bootstrap_kills_detached_descendant_before_return"),
    ("root-unit-parent-death", "release_publish_entry_systemd_test", "PublisherEntrySystemdTest", "test_root_bootstrap_manager_reaps_child_after_parent_death"),
    ("root-main-credential", "release_publish_entry_offline_test", "PublisherEntryOfflineTest", "test_main_passes_credential_to_real_bootstrap"),
}

stage_files = {
    "src": ("release_generator_bootstrap.sh", "release_manager_bus.py", "release_manager_lifecycle.py",
            "release_publish_entry.py", "release_supervisor_exec.py"),
    "tests": ("release_manager_proxy_test.py", "release_manager_qualification.py",
              "release_manager_scenario_controller.py", "release_manager_systemd_test.py",
              "release_publish_entry_offline_test.py", "release_publish_entry_systemd_test.py",
              "release_publish_entry_test.py"),
}


def stage_and_execute():
    if os.geteuid() != 0:
        raise SystemExit("qualification staging requires root")
    source = root.parent
    with tempfile.TemporaryDirectory(prefix="velnor-qualification-source-", dir="/run") as directory:
        destination = Path(directory)
        destination.chmod(0o755)
        source_fd = os.open(source, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC)
        try:
            for subdirectory, names in stage_files.items():
                target = destination / subdirectory
                target.mkdir(mode=0o755)
                parent = os.open(subdirectory, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC,
                                 dir_fd=source_fd)
                try:
                    for name in names:
                        descriptor = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC, dir_fd=parent)
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
        subprocess.run(["/usr/bin/python3", "-I", "-S", "-B",
                        str(destination / "tests/release_manager_qualification.py"), "--execute-staged"],
                       stdin=subprocess.DEVNULL, env={"PATH": "/usr/bin:/bin", "HOME": "/nonexistent", "LANG": "C"},
                       timeout=240, check=True)


def execute():
    if {(name, *spec) for name, spec in scenarios.items()} != capability_v1:
        raise SystemExit("qualification scenario inventory mismatch")
    allowed = {"PATH", "HOME", "LANG", "LC_CTYPE", "MAIL", "LOGNAME", "USER", "SHELL", "TERM",
               "SUDO_COMMAND", "SUDO_USER", "SUDO_UID", "SUDO_GID"}
    if set(os.environ) - allowed:
        raise SystemExit("unexpected privileged qualification environment")
    spec = importlib.util.spec_from_file_location("scenario_controller", root / "release_manager_scenario_controller.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    module.run_fixed()


def execute_scenarios():
    suites = []
    for module_name, class_name, method in scenarios.values():
        spec = importlib.util.spec_from_file_location(module_name, root / (module_name + ".py"))
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        suite = unittest.defaultTestLoader.loadTestsFromName(class_name + "." + method, module)
        if suite.countTestCases() != 1:
            raise SystemExit("qualification scenario missing")
        suites.append(suite)
    result = unittest.TextTestRunner(verbosity=1).run(unittest.TestSuite(suites))
    if not result.wasSuccessful() or result.testsRun != len(scenarios) or result.skipped or result.expectedFailures:
        raise SystemExit("supervisor qualification failed")


def main():
    if sys.argv[1:] == ["--execute-scenarios"]:
        if os.geteuid() != 0:
            raise SystemExit("scenario execution requires root")
        spec = importlib.util.spec_from_file_location("scenario_controller", root / "release_manager_scenario_controller.py")
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        module.run(execute_scenarios, os.environ["VELNOR_SCENARIO_LEDGER"])
        return
    if sys.argv[1:] == ["--execute"]:
        stage_and_execute()
        return
    if sys.argv[1:] == ["--execute-staged"]:
        execute()
        return
    if sys.argv[1:]:
        raise SystemExit("invalid qualification request")
    release = Path("/etc/os-release").read_text(encoding="utf-8").splitlines()
    if (sys.platform != "linux" or os.uname().machine != "x86_64"
            or "ID=ubuntu" not in release or 'VERSION_ID="24.04"' not in release):
        raise SystemExit("unexpected operating system image")
    head = os.environ["VELNOR_CHECK_HEAD"]
    check = os.environ["VELNOR_CHECK_ID"]
    platform = os.environ["VELNOR_CHECK_PLATFORM"]
    if not re.fullmatch(r"[0-9a-f]{40}", head) or check != "hosted-publisher-supervisor" or platform != "linux_x64":
        raise SystemExit("invalid qualification identity")
    image_os = os.environ.get("ImageOS", "unavailable")
    image_version = os.environ.get("ImageVersion", "unavailable")
    if (not re.fullmatch(r"[A-Za-z0-9_.-]{1,128}", image_os)
            or not re.fullmatch(r"[A-Za-z0-9_.-]{1,128}", image_version)):
        raise SystemExit("hosted image identity unavailable")
    systemd = subprocess.run(["/usr/bin/systemctl", "--version"], stdin=subprocess.DEVNULL,
                             stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, timeout=5, check=True).stdout
    first = systemd.splitlines()[0].decode("ascii")
    if not re.fullmatch(r"systemd [0-9]+(?: \([A-Za-z0-9_.+~:-]+\))?", first):
        raise SystemExit("systemd version unavailable")
    print("supervisor environment image_os=" + image_os + " image_version=" + image_version
          + " kernel=" + os.uname().release + " " + first, flush=True)
    subprocess.run(["/usr/bin/sudo", "-n", "/usr/bin/python3", "-I", "-S", "-B", str(Path(__file__).resolve()), "--execute"],
                   stdin=subprocess.DEVNULL, env={"PATH": "/usr/bin:/bin", "HOME": "/nonexistent", "LANG": "C"},
                   timeout=240, check=True)
    evidence = {"schema": 1, "source": "mise-task-v1", "head": head, "check_id": check, "platform": platform,
                "scenarios": [{"id": name, "executed": True, "status": "passed"} for name in sorted(scenarios)]}
    with open("hosted-publisher-supervisor-evidence.json", "x", encoding="utf-8") as output:
        json.dump(evidence, output, separators=(",", ":"))


if __name__ == "__main__":
    main()

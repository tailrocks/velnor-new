# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'src'))

import importlib.util
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


sys.path.insert(0, str(Path(__file__).resolve().parent))
from release_qualification_source import stage


root = Path(__file__).resolve().parent
scenarios = {
    "escaped-descendant": ("release_manager_systemd_test", "RealManagerTest", "test_detached_term_resistant_descendant_is_gone_before_completion"),
    "malformed-cgroup-injected": ("release_manager_systemd_test", "RealManagerTest", "test_malformed_kernel_cgroup_state_cannot_verify_cleanup"),
    "delivery": ("release_manager_systemd_test", "RealManagerTest", "test_real_manager_delivers_and_removes_owned_unit_dropin"),
    "queued-timeout": ("release_manager_systemd_test", "RealManagerTest", "test_real_queued_start_times_out_then_stop_converges"),
    "parent-death-queued": ("release_manager_systemd_test", "RealManagerTest", "test_parent_death_while_accepted_start_is_queued"),
    "parent-death-delivered": ("release_manager_systemd_test", "RealManagerTest", "test_manager_runtime_reaps_delivered_child_after_supervisor_death"),
    "staging-symlink-refusal": ("release_manager_systemd_test", "RealManagerTest", "test_root_owned_staging_rejects_preexisting_symlink"),
    "cleanup-policy-refusal": ("release_manager_systemd_test", "RealManagerTest", "test_dependent_cleanup_policy_rejects_effective_overrides"),
    "cleanup-policy-watch-drain": ("release_manager_systemd_test", "RealManagerTest", "test_refused_policy_never_delivers_frame_and_watch_drains"),
    "failed-unit-collected": ("release_manager_systemd_test", "RealManagerTest", "test_failed_dependent_unit_is_observed_before_release_then_collected"),
    "fake-broker-refusal": ("release_manager_proxy_test", "RealReplyLossTest", "test_production_constructor_rejects_inherited_fake_broker"),
    "reply-loss-late-start": ("release_manager_proxy_test", "RealReplyLossTest", "test_accepted_start_runs_late_after_reply_loss_without_input"),
    "reply-loss-unref": ("release_manager_proxy_test", "RealReplyLossTest", "test_lost_unref_reply_never_reports_success"),
    "root-original-budget": ("release_publish_entry_test", "PublisherEntryTest", "test_deadline_uses_the_original_job_budget"),
    "root-context-modes": ("release_publish_entry_test", "PublisherEntryTest", "test_context_accepts_only_matching_release_modes"),
    "root-context-rejections": ("release_publish_entry_test", "PublisherEntryTest", "test_context_rejects_unapproved_identity_and_credentials"),
    "root-context-ref": ("release_publish_entry_test", "PublisherEntryTest", "test_context_rejects_invalid_branch_even_when_workflow_ref_matches"),
    "root-binary-custody": ("release_publish_entry_test", "PublisherEntryTest", "test_executable_requires_exact_root_owned_regular_bytes"),
    "root-unit-clean": ("release_publish_entry_systemd_test", "PublisherEntrySystemdTest", "test_root_bootstrap_finishes_only_after_unit_quiesces"),
    "root-unit-descendant": ("release_publish_entry_systemd_test", "PublisherEntrySystemdTest", "test_root_bootstrap_kills_detached_descendant_before_return"),
    "root-unit-parent-death": ("release_publish_entry_systemd_test", "PublisherEntrySystemdTest", "test_root_bootstrap_manager_reaps_child_after_parent_death"),
    "root-main-credential": ("release_publish_entry_offline_test", "PublisherEntryOfflineTest", "test_main_passes_credential_to_real_bootstrap"),
    "root-coordinator-credential-boundary": ("release_coordinator_exec_test", "CoordinatorChannelTest", "test_coordinator_rejects_publishing_credentials_in_bootstrap"),
    "root-retained-credential-boundary": ("release_retained_entry_systemd_test", "RetainedEntryTest", "test_parent_strips_publish_credentials_and_reaps_units"),
    "root-coordinator-channel": ("release_coordinator_exec_test", "CoordinatorChannelTest", "test_root_coordinator_keeps_bidirectional_channel"),
    "root-bound-unit-parent-death": ("release_coordinator_exec_test", "CoordinatorChannelTest", "test_bound_unit_stops_when_supervisor_unit_dies"),
    "root-one-shot-reactivation": ("release_coordinator_exec_test", "CoordinatorChannelTest", "test_preclaim_rejects_late_reactivation"),
    "root-source-process-isolation": ("release_retained_entry_systemd_test", "RetainedEntryTest", "test_source_unit_uses_private_tmp_and_denies_unix_sockets"),
    "root-source-network-isolation": ("release_retained_entry_systemd_test", "RetainedEntryTest", "test_source_unit_denies_network_when_requested"),
}


capability_v1 = {
    ("escaped-descendant", "release_manager_systemd_test", "RealManagerTest", "test_detached_term_resistant_descendant_is_gone_before_completion"),
    ("malformed-cgroup-injected", "release_manager_systemd_test", "RealManagerTest", "test_malformed_kernel_cgroup_state_cannot_verify_cleanup"),
    ("delivery", "release_manager_systemd_test", "RealManagerTest", "test_real_manager_delivers_and_removes_owned_unit_dropin"),
    ("queued-timeout", "release_manager_systemd_test", "RealManagerTest", "test_real_queued_start_times_out_then_stop_converges"),
    ("parent-death-queued", "release_manager_systemd_test", "RealManagerTest", "test_parent_death_while_accepted_start_is_queued"),
    ("parent-death-delivered", "release_manager_systemd_test", "RealManagerTest", "test_manager_runtime_reaps_delivered_child_after_supervisor_death"),
    ("staging-symlink-refusal", "release_manager_systemd_test", "RealManagerTest", "test_root_owned_staging_rejects_preexisting_symlink"),
    ("cleanup-policy-refusal", "release_manager_systemd_test", "RealManagerTest", "test_dependent_cleanup_policy_rejects_effective_overrides"),
    ("cleanup-policy-watch-drain", "release_manager_systemd_test", "RealManagerTest", "test_refused_policy_never_delivers_frame_and_watch_drains"),
    ("failed-unit-collected", "release_manager_systemd_test", "RealManagerTest", "test_failed_dependent_unit_is_observed_before_release_then_collected"),
    ("fake-broker-refusal", "release_manager_proxy_test", "RealReplyLossTest", "test_production_constructor_rejects_inherited_fake_broker"),
    ("reply-loss-late-start", "release_manager_proxy_test", "RealReplyLossTest", "test_accepted_start_runs_late_after_reply_loss_without_input"),
    ("reply-loss-unref", "release_manager_proxy_test", "RealReplyLossTest", "test_lost_unref_reply_never_reports_success"),
    ("root-original-budget", "release_publish_entry_test", "PublisherEntryTest", "test_deadline_uses_the_original_job_budget"),
    ("root-context-modes", "release_publish_entry_test", "PublisherEntryTest", "test_context_accepts_only_matching_release_modes"),
    ("root-context-rejections", "release_publish_entry_test", "PublisherEntryTest", "test_context_rejects_unapproved_identity_and_credentials"),
    ("root-context-ref", "release_publish_entry_test", "PublisherEntryTest", "test_context_rejects_invalid_branch_even_when_workflow_ref_matches"),
    ("root-binary-custody", "release_publish_entry_test", "PublisherEntryTest", "test_executable_requires_exact_root_owned_regular_bytes"),
    ("root-unit-clean", "release_publish_entry_systemd_test", "PublisherEntrySystemdTest", "test_root_bootstrap_finishes_only_after_unit_quiesces"),
    ("root-unit-descendant", "release_publish_entry_systemd_test", "PublisherEntrySystemdTest", "test_root_bootstrap_kills_detached_descendant_before_return"),
    ("root-unit-parent-death", "release_publish_entry_systemd_test", "PublisherEntrySystemdTest", "test_root_bootstrap_manager_reaps_child_after_parent_death"),
    ("root-main-credential", "release_publish_entry_offline_test", "PublisherEntryOfflineTest", "test_main_passes_credential_to_real_bootstrap"),
    ("root-coordinator-credential-boundary", "release_coordinator_exec_test", "CoordinatorChannelTest", "test_coordinator_rejects_publishing_credentials_in_bootstrap"),
    ("root-retained-credential-boundary", "release_retained_entry_systemd_test", "RetainedEntryTest", "test_parent_strips_publish_credentials_and_reaps_units"),
    ("root-coordinator-channel", "release_coordinator_exec_test", "CoordinatorChannelTest", "test_root_coordinator_keeps_bidirectional_channel"),
    ("root-bound-unit-parent-death", "release_coordinator_exec_test", "CoordinatorChannelTest", "test_bound_unit_stops_when_supervisor_unit_dies"),
    ("root-one-shot-reactivation", "release_coordinator_exec_test", "CoordinatorChannelTest", "test_preclaim_rejects_late_reactivation"),
    ("root-source-process-isolation", "release_retained_entry_systemd_test", "RetainedEntryTest", "test_source_unit_uses_private_tmp_and_denies_unix_sockets"),
    ("root-source-network-isolation", "release_retained_entry_systemd_test", "RetainedEntryTest", "test_source_unit_denies_network_when_requested"),
}


def stage_and_execute():
    if os.geteuid() != 0:
        raise SystemExit("qualification staging requires root")
    source = root.parent
    with tempfile.TemporaryDirectory(prefix="velnor-qualification-source-", dir="/run") as directory:
        destination = Path(directory)
        destination.chmod(0o755)
        stage(source, destination)
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
    raise SystemExit("invalid qualification request")


if __name__ == "__main__":
    main()

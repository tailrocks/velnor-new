# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import json
import os
import pathlib
import signal
import subprocess
import time
import unittest

from test_build_runner_images_fixture import ImageBuildFixture, volume_mount


class BuildRunnerImagesTests(unittest.TestCase):
    def setUp(self):
        self.fixture = ImageBuildFixture()

    def tearDown(self):
        self.fixture.close()

    def assert_rejected_before_docker(self, result):
        self.assertNotEqual(0, result.returncode, result.stdout)
        self.assertEqual([], self.fixture.events())

    def test_public_command_builds_and_smokes_source_bound_pair(self):
        result = self.fixture.run_builder()
        self.assertEqual(0, result.returncode, result.stdout + result.stderr)
        builds = [event for event in self.fixture.events() if event["kind"] == "build"]
        self.assertEqual(2, len(builds))
        tags = {event["tag"] for event in builds}
        self.assertEqual(2, len(tags))
        self.assertTrue(all(self.fixture.source_sha[:12] in tag for tag in tags))
        self.assertFalse(any(tag in {"velnor-runner:ubuntu-26.04-2.338.0", "velnor-dind:29.9.1"} for tag in tags))
        for event in builds:
            self.assertIn("--platform", event["args"])
            self.assertIn("linux/amd64", event["args"])
            self.assertIn(f"org.opencontainers.image.revision={self.fixture.source_sha}", event["args"])
        self.assertIn("inner_docker_id=inner:daemon:id", result.stdout)
        self.assertIn("outer_docker_id=outer:daemon:id", result.stdout)
        events = self.fixture.events()
        dind_event = next(event for event in events if event["kind"] == "container-start")
        client_event = next(
            event
            for event in events
            if event["kind"] == "run"
            and event["image"].startswith("velnor-runner:")
            and "info" in event["command"]
        )
        dind_socket = volume_mount(dind_event["args"], "/run")
        runner_socket = volume_mount(client_event["args"], "/run")
        self.assertEqual("volume", dind_socket.get("type"))
        self.assertEqual(dind_socket, runner_socket)
        kinds = [event["kind"] for event in events]
        self.assertIn("container-remove", kinds)
        self.assertEqual(2, kinds.count("volume-remove"))

    def test_rejects_wrong_source_revision_before_docker(self):
        result = self.fixture.run_builder("f" * 40)
        self.assert_rejected_before_docker(result)

    def test_rejects_dirty_tracked_source_before_docker(self):
        target = self.fixture.repository / "images/runner/ubuntu-26.04/Dockerfile"
        target.write_bytes(target.read_bytes() + b"\nchanged\n")
        result = self.fixture.run_builder()
        self.assert_rejected_before_docker(result)

    def test_rejects_untracked_source_before_docker(self):
        (self.fixture.repository / "images/runner/ubuntu-26.04/untracked").write_text("source")
        result = self.fixture.run_builder()
        self.assert_rejected_before_docker(result)

    def test_rejects_failed_build(self):
        self.fixture.mode = "build-failure"
        result = self.fixture.run_builder()
        self.assertNotEqual(0, result.returncode)
        self.assertIn("injected build failure", result.stderr)

    def test_rejects_wrong_image_architecture(self):
        self.fixture.mode = "wrong-architecture"
        result = self.fixture.run_builder()
        self.assertNotEqual(0, result.returncode)
        self.assertIn("linux/amd64", result.stderr)

    def test_rejects_image_with_wrong_source_label(self):
        self.fixture.mode = "wrong-source-label"
        result = self.fixture.run_builder()
        self.assertNotEqual(0, result.returncode)
        self.assertIn("image source label", result.stderr)

    def test_rejects_wrong_listener_version(self):
        self.fixture.mode = "wrong-listener-version"
        result = self.fixture.run_builder()
        self.assertNotEqual(0, result.returncode)
        self.assertIn("Runner.Listener", result.stderr)

    def test_rejects_failed_socket_smoke(self):
        self.fixture.mode = "failed-smoke"
        result = self.fixture.run_builder()
        self.assertNotEqual(0, result.returncode)
        self.assertIn("different Docker daemon", result.stderr)

    def test_rejects_tracked_source_modified_during_build(self):
        self.fixture.mode = "source-modified-during-build"
        result = self.fixture.run_builder()
        self.assertNotEqual(0, result.returncode)
        self.assertIn("source tree differs from HEAD", result.stderr)

    def test_socket_query_deadline_cleans_owned_resources_and_preserves_sentinel(self):
        self.fixture.close()
        self.fixture = ImageBuildFixture(mode="blocked-socket-query")
        result = self.fixture.run_builder_with_watchdog(7)
        events = self.fixture.events()
        state = json.loads(self.fixture.state_path.read_text())
        evidence_directory = os.environ.get("VELNOR_IMAGE_BUILDER_EVIDENCE_DIR")
        if evidence_directory:
            evidence = pathlib.Path(evidence_directory)
            evidence.mkdir(parents=True, exist_ok=True)
            (evidence / "blocked-query.json").write_text(
                json.dumps(
                    {
                        "returncode": result.returncode,
                        "elapsed_seconds": result.elapsed,
                        "stdout": result.stdout,
                        "stderr": result.stderr,
                        "events": events,
                        "docker_state": state,
                    },
                    indent=2,
                    sort_keys=True,
                )
                + "\n"
            )
        self.assertIn("query-blocked", [event["kind"] for event in events])
        self.assertNotEqual(0, result.returncode)
        self.assertLess(result.elapsed, 8)
        self.assertIn("deadline", result.stderr.lower())
        client = next(event["name"] for event in events if event["kind"] == "client-created")
        removals = [event["name"] for event in events if event["kind"] == "container-remove"]
        self.assertIn(client, removals)
        self.assertTrue(any(name.startswith("velnor-image-smoke-") for name in removals))
        self.assertEqual(2, sum(event["kind"] == "volume-remove" for event in events))
        self.assertEqual({"unrelated-sentinel"}, set(state["containers"]))
        self.assertEqual({}, state["volumes"])


if __name__ == "__main__":
    unittest.main()

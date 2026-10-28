# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import json
import os
import pathlib
import signal
import shutil
import subprocess
import tempfile
import textwrap
import time


ROOT = pathlib.Path(__file__).resolve().parents[2]
BUILDER = "images/resource-probe/build-runner-images.sh"
DOCKER_STUB = textwrap.dedent(
    """\
    #!/usr/bin/env python3
    import json
    import os
    import pathlib
    import sys
    import time

    args = sys.argv[1:]
    mode = os.environ["DOCKER_FIXTURE_MODE"]
    state_path = pathlib.Path(os.environ["DOCKER_FIXTURE_STATE"])
    log_path = pathlib.Path(os.environ["DOCKER_FIXTURE_LOG"])
    repository = pathlib.Path(os.environ["DOCKER_FIXTURE_REPO"])
    outer_id = os.environ["DOCKER_FIXTURE_OUTER_ID"]
    inner_id = os.environ["DOCKER_FIXTURE_INNER_ID"]
    state = json.loads(state_path.read_text()) if state_path.exists() else {
        "images": {}, "containers": {}, "volumes": {}, "mutated": False
    }

    def record(kind, **values):
        with log_path.open("a") as stream:
            stream.write(json.dumps({"kind": kind, **values}) + "\\n")

    def labels_from(values):
        labels = {}
        index = 0
        while index < len(values):
            value = values[index]
            if value == "--label" and index + 1 < len(values):
                key, label = values[index + 1].split("=", 1)
                labels[key] = label
                index += 2
            elif value.startswith("--label="):
                key, label = value[8:].split("=", 1)
                labels[key] = label
                index += 1
            else:
                index += 1
        return labels

    def mount_at(values, target):
        mounts = []
        index = 0
        while index < len(values):
            value = values[index]
            if value == "--mount" and index + 1 < len(values):
                mounts.append(values[index + 1])
                index += 2
            elif value.startswith("--mount="):
                mounts.append(value[8:])
                index += 1
            else:
                index += 1
        for mount in mounts:
            fields = dict(part.split("=", 1) for part in mount.split(",") if "=" in part)
            if fields.get("target") == target:
                return fields
        return {}

    def finish(code=0, output="", error=""):
        state_path.write_text(json.dumps(state))
        if output:
            print(output)
        if error:
            print(error, file=sys.stderr)
        raise SystemExit(code)

    if args[:1] == ["build"]:
        tag = args[args.index("--tag") + 1]
        context = args[-1]
        record("build", args=args, tag=tag, context=context)
        labels = labels_from(args)
        state["images"][tag] = labels
        if mode == "build-failure" and tag.startswith("velnor-runner:"):
            finish(17, error="injected build failure")
        if mode == "source-modified-during-build" and not state["mutated"]:
            target = repository / context / "Dockerfile"
            target.write_bytes(target.read_bytes() + b"\\nfixture mutation\\n")
            state["mutated"] = True
        finish()

    if args[:1] == ["version"]:
        record("outer-version", args=args)
        finish(output="29.1.1")

    if args[:2] == ["image", "inspect"]:
        tag = args[-1]
        labels = state["images"].get(tag, {})
        architecture = "arm64" if mode == "wrong-architecture" else "amd64"
        revision = labels.get("org.opencontainers.image.revision", "")
        if mode == "wrong-source-label":
            revision = "f" * 40
        run_id = labels.get("io.velnor.image-builder.run", "")
        record("image-inspect", tag=tag)
        image_id = "sha256:" + ("1" if tag.startswith("velnor-runner:") else "2") * 64
        finish(output=f"linux|{architecture}|{image_id}|{revision}|{run_id}")

    if args[:2] == ["volume", "create"]:
        name = args[-1]
        state["volumes"][name] = labels_from(args)
        record("volume-create", name=name, labels=state["volumes"][name])
        finish(output=name)

    if args[:1] == ["run"]:
        if "--detach" in args:
            name = args[args.index("--name") + 1]
            state["containers"][name] = {
                "labels": labels_from(args),
                "running": True,
                "socket_mount": mount_at(args, "/run"),
            }
            record("container-start", name=name, args=args)
            finish(output="c" * 64)
        entrypoint = args[args.index("--entrypoint") + 1]
        image = args[args.index("--entrypoint") + 2]
        command = args[args.index(image) + 1:]
        record("run", image=image, entrypoint=entrypoint, command=command, args=args)
        if entrypoint == "/home/runner/bin/Runner.Listener":
            version = "2.337.0" if mode == "wrong-listener-version" else "2.338.0"
            finish(output=version)
        if entrypoint == "/usr/local/bin/docker" and command == ["--version"]:
            finish(output="Docker version 29.9.1, build fixture")
        if entrypoint == "/usr/local/bin/dockerd" and command == ["--version"]:
            finish(output="Docker version 29.9.1, build fixture")
        if entrypoint == "/usr/local/bin/docker" and "info" in command:
            if image.startswith("velnor-runner:"):
                name = args[args.index("--name") + 1]
                state["containers"][name] = {
                    "labels": labels_from(args),
                    "running": True,
                    "socket_mount": mount_at(args, "/run"),
                }
                record("client-created", name=name)
                if mode == "blocked-socket-query":
                    state_path.write_text(json.dumps(state))
                    record("query-blocked", name=name)
                    while True:
                        time.sleep(60)
                runner_mount = mount_at(args, "/run")
                dind_mount = next(
                    (container["socket_mount"] for container in state["containers"].values()),
                    {},
                )
                reaches_inner = (
                    runner_mount.get("type") == "volume"
                    and runner_mount.get("source") == dind_mount.get("source")
                    and mode != "failed-smoke"
                )
                reported_id = inner_id if reaches_inner else outer_id
            else:
                reported_id = inner_id
            finish(output=reported_id)
        finish(64, error="unexpected docker run invocation")

    if args[:1] == ["exec"]:
        record("exec", args=args)
        finish(output=inner_id)

    if args[:1] == ["info"]:
        record("outer-info", args=args)
        finish(output=outer_id)

    if args[:2] == ["container", "inspect"]:
        name = args[-1]
        container = state["containers"].get(name)
        if container is None:
            finish(1, error="container not found")
        template = args[args.index("--format") + 1]
        output = "true" if ".State.Running" in template else container["labels"].get("io.velnor.image-builder.run", "")
        finish(output=output)

    if args[:2] == ["volume", "inspect"]:
        name = args[-1]
        volume = state["volumes"].get(name)
        if volume is None:
            finish(1, error="volume not found")
        finish(output=volume.get("io.velnor.image-builder.run", ""))

    if args[:1] == ["rm"]:
        name = args[-1]
        state["containers"].pop(name, None)
        record("container-remove", name=name)
        finish()

    if args[:2] == ["volume", "rm"]:
        name = args[-1]
        state["volumes"].pop(name, None)
        record("volume-remove", name=name)
        finish()

    finish(64, error="unexpected docker invocation: " + " ".join(args))
    """
)


def volume_mount(arguments, target):
    for index, value in enumerate(arguments):
        if value == "--mount" and index + 1 < len(arguments):
            mount = arguments[index + 1]
        elif value.startswith("--mount="):
            mount = value[8:]
        else:
            continue
        fields = dict(part.split("=", 1) for part in mount.split(",") if "=" in part)
        if fields.get("target") == target:
            return fields
    return {}


class ImageBuildFixture:
    def __init__(self, mode="success"):
        self.temporary = tempfile.TemporaryDirectory(prefix="velnor-image-builder-test-")
        self.directory = pathlib.Path(self.temporary.name)
        self.repository = self.directory / "repo"
        self.bin_directory = self.directory / "bin"
        self.repository.mkdir()
        self.bin_directory.mkdir()
        shutil.copytree(ROOT / "images/runner/ubuntu-26.04", self.repository / "images/runner/ubuntu-26.04")
        shutil.copytree(ROOT / "images/dind", self.repository / "images/dind")
        (self.repository / ".velnor").mkdir()
        shutil.copy2(ROOT / ".velnor/runner.lock", self.repository / ".velnor/runner.lock")
        self.docker = self.bin_directory / "docker"
        self.docker.write_text(DOCKER_STUB)
        self.docker.chmod(0o755)
        self.log_path = self.directory / "docker.jsonl"
        self.state_path = self.directory / "docker-state.json"
        self.mode = mode
        if mode == "blocked-socket-query":
            self.state_path.write_text(
                json.dumps(
                    {
                        "images": {},
                        "containers": {
                            "unrelated-sentinel": {
                                "labels": {"io.velnor.image-builder.run": "other-run"},
                                "running": True,
                            }
                        },
                        "volumes": {},
                        "mutated": False,
                    }
                )
            )
        source_builder = ROOT / BUILDER
        if source_builder.is_file():
            target = self.repository / BUILDER
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source_builder, target)
        self.git("init", "-q")
        self.git("config", "user.name", "Velnor test")
        self.git("config", "user.email", "velnor-test@example.invalid")
        self.git("add", "-A")
        self.git("commit", "-q", "-m", "fixture source")
        self.source_sha = self.git("rev-parse", "HEAD").strip()

    def git(self, *arguments):
        result = subprocess.run(
            ["git", *arguments], cwd=self.repository, text=True, capture_output=True
        )
        if result.returncode:
            raise AssertionError(result.stdout + result.stderr)
        return result.stdout

    def close(self):
        self.temporary.cleanup()

    def events(self):
        if not self.log_path.exists():
            return []
        return [json.loads(line) for line in self.log_path.read_text().splitlines()]

    def run_builder(self, source_sha=None):
        environment = os.environ.copy()
        environment.update(
            {
                "PATH": f"{self.bin_directory}:{environment['PATH']}",
                "VELNOR_SOURCE_REVISION": source_sha or self.source_sha,
                "DOCKER_FIXTURE_MODE": self.mode,
                "DOCKER_FIXTURE_LOG": str(self.log_path),
                "DOCKER_FIXTURE_STATE": str(self.state_path),
                "DOCKER_FIXTURE_REPO": str(self.repository),
                "DOCKER_FIXTURE_OUTER_ID": "outer:daemon:id",
                "DOCKER_FIXTURE_INNER_ID": "inner:daemon:id",
            }
        )
        return subprocess.run(
            ["bash", BUILDER],
            cwd=self.repository,
            env=environment,
            text=True,
            capture_output=True,
        )

    def run_builder_with_watchdog(self, timeout):
        environment = os.environ.copy()
        environment.update(
            {
                "PATH": f"{self.bin_directory}:{environment['PATH']}",
                "VELNOR_SOURCE_REVISION": self.source_sha,
                "DOCKER_FIXTURE_MODE": self.mode,
                "DOCKER_FIXTURE_LOG": str(self.log_path),
                "DOCKER_FIXTURE_STATE": str(self.state_path),
                "DOCKER_FIXTURE_REPO": str(self.repository),
                "DOCKER_FIXTURE_OUTER_ID": "outer:daemon:id",
                "DOCKER_FIXTURE_INNER_ID": "inner:daemon:id",
            }
        )
        command = ["bash", BUILDER]
        started = time.monotonic()
        process = subprocess.Popen(
            command,
            cwd=self.repository,
            env=environment,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            start_new_session=True,
        )
        while process.poll() is None and time.monotonic() - started < timeout:
            time.sleep(0.05)
        if process.poll() is None:
            os.killpg(process.pid, signal.SIGTERM)
            try:
                stdout, stderr = process.communicate(timeout=0.5)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                stdout, stderr = process.communicate()
        else:
            stdout, stderr = process.communicate()
        result = subprocess.CompletedProcess(command, process.returncode, stdout, stderr)
        result.elapsed = time.monotonic() - started
        return result

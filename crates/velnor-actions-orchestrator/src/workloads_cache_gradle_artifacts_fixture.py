"""Offline transport fixture for the Gradle provenance proof."""
import hashlib
import json
import os
import pathlib
import sys
import tempfile


engine = {}
exec(sys.argv[1], engine)

JAR = b"closed public jar fixture"
POM = b"""<?xml version='1.0'?>
<project>
  <groupId>test.group</groupId>
  <artifactId>fixture</artifactId>
  <version>1.0.0</version>
</project>
"""
descriptor = {
    "jar_algorithm": "sha256",
    "jar_checksum": hashlib.sha256(JAR).hexdigest(),
    "jar_sha256": hashlib.sha256(JAR).hexdigest(),
    "pom_algorithm": "sha256",
    "pom_checksum": hashlib.sha256(POM).hexdigest(),
}
engine["CLOSED"] = {("test.group", "fixture", "1.0.0"): descriptor}
source = {"group": "test.group", "module": "fixture", "version": "1.0.0", **descriptor}
engine["CACHE_SHA1"] = {("test.group", "fixture", "1.0.0"): (
    hashlib.sha1(JAR).hexdigest(), hashlib.sha1(POM).hexdigest())}
responses = {}
requests = []
mode = "ok"


def fill(path, body):
    responses[engine["coordinate_path"](source, path)] = body


fill(".jar.sha256", (descriptor["jar_checksum"] + "\n").encode())
fill(".pom.sha256", (descriptor["pom_checksum"] + "\n").encode())
fill(".jar", JAR)
fill(".pom", POM)


class Response:
    def __init__(self, body, status=200):
        self.body = body
        self.status = status
        self.offset = 0

    def getheader(self, name):
        return None

    def read(self, maximum):
        chunk = self.body[self.offset:self.offset + maximum]
        self.offset += len(chunk)
        return chunk


class Connection:
    def __init__(self, host, timeout, context=None):
        assert host == engine["HOST"]

    def request(self, method, path, headers):
        assert method == "GET"
        assert "authorization" not in {key.lower() for key in headers}
        assert "cookie" not in {key.lower() for key in headers}
        assert "token" not in str(headers).lower()
        requests.append(path)
        self.path = path

    def getresponse(self):
        status = 302 if mode == "redirect" else 503 if mode == "error" else 200
        return Response(responses[self.path], status)

    def close(self):
        pass


engine["http"].client.HTTPSConnection = Connection
os.environ["MAVEN_TOKEN"] = "ambient-secret-sentinel"
with tempfile.TemporaryDirectory() as temp:
    runner = pathlib.Path(temp).resolve()
    os.environ["RUNNER_TEMP"] = str(runner)
    os.environ["GRADLE_USER_HOME"] = str(runner / "velnor/native/gradle")
    os.environ["VELNOR_GRADLE_ARTIFACT_HOME_ROLE"] = "consumer"
    actual = engine["qualify_sources"]([source])
    assert len(actual) == 1
    entry = actual[0]
    assert entry["group"] == "test.group"
    assert entry["module"] == "fixture"
    assert entry["version"] == "1.0.0"
    assert entry["sha256"] == descriptor["jar_sha256"]
    assert pathlib.Path(entry["path"]).is_file()
    assert requests == [
        "/maven2/test/group/fixture/1.0.0/fixture-1.0.0.jar.sha256",
        "/maven2/test/group/fixture/1.0.0/fixture-1.0.0.pom.sha256",
        "/maven2/test/group/fixture/1.0.0/fixture-1.0.0.jar",
        "/maven2/test/group/fixture/1.0.0/fixture-1.0.0.pom",
    ]
    assert "MAVEN_TOKEN" not in json.dumps(actual)
    requests.clear()
    engine["qualify_sources"]([source])
    assert requests == [
        "/maven2/test/group/fixture/1.0.0/fixture-1.0.0.jar.sha256",
        "/maven2/test/group/fixture/1.0.0/fixture-1.0.0.pom.sha256",
    ]
    pathlib.Path(entry["path"]).write_bytes(b"corrupt restored cache")
    requests.clear()
    engine["qualify_sources"]([source])
    assert pathlib.Path(entry["path"]).read_bytes() == JAR
    assert requests == [
        "/maven2/test/group/fixture/1.0.0/fixture-1.0.0.jar.sha256",
        "/maven2/test/group/fixture/1.0.0/fixture-1.0.0.pom.sha256",
        "/maven2/test/group/fixture/1.0.0/fixture-1.0.0.jar",
    ]
    for failure_mode in ("redirect", "error"):
        mode = failure_mode
        try:
            engine["qualify_sources"]([source])
        except ValueError:
            pass
        else:
            raise AssertionError("non-success Central response accepted")
    mode = "ok"
    try:
        engine["qualify_sources"]([dict(source, group="private.example")])
    except ValueError:
        pass
    else:
        raise AssertionError("arbitrary coordinate accepted")

    os.environ["VELNOR_GRADLE_ARTIFACT_HOME_ROLE"] = "producer"
    try:
        engine["safe_home"]()
    except ValueError:
        pass
    else:
        raise AssertionError("consumer home accepted for producer role")
    os.environ["VELNOR_GRADLE_ARTIFACT_HOME_ROLE"] = "consumer"
    os.environ["GRADLE_USER_HOME"] = str(runner / "velnor/native/wrong")
    try:
        engine["safe_home"]()
    except ValueError:
        pass
    else:
        raise AssertionError("wrong role root accepted")

    with tempfile.TemporaryDirectory() as producer_temp:
        producer_runner = pathlib.Path(producer_temp).resolve()
        producer_home = producer_runner / "velnor/native/gradle-producer/gradle-home"
        os.environ["RUNNER_TEMP"] = str(producer_runner)
        os.environ["GRADLE_USER_HOME"] = str(producer_home)
        os.environ["VELNOR_GRADLE_ARTIFACT_HOME_ROLE"] = "producer"
        producer_entries = engine["qualify_sources"]([source])
        assert str(producer_home) in producer_entries[0]["path"]
        os.environ["VELNOR_GRADLE_ARTIFACT_HOME_ROLE"] = "consumer"
        try:
            engine["safe_home"]()
        except ValueError:
            pass
        else:
            raise AssertionError("producer home accepted for consumer role")

    with tempfile.TemporaryDirectory() as oversized_temp:
        oversized_runner = pathlib.Path(oversized_temp).resolve()
        os.environ["RUNNER_TEMP"] = str(oversized_runner)
        os.environ["GRADLE_USER_HOME"] = str(oversized_runner / "velnor/native/gradle")
        os.environ["VELNOR_GRADLE_ARTIFACT_HOME_ROLE"] = "consumer"
        oversized = (oversized_runner / "velnor/native/gradle/caches/modules-2/files-2.1"
                     / "test.group/fixture/1.0.0"
                     / hashlib.sha1(JAR).hexdigest() / "fixture-1.0.0.jar")
        oversized.parent.mkdir(parents=True)
        with oversized.open("wb") as output:
            output.truncate(engine["MAX_JAR"] + 1)
        try:
            engine["qualify_sources"]([source])
        except ValueError:
            pass
        else:
            raise AssertionError("oversized cached artifact accepted")

    with tempfile.TemporaryDirectory() as ancestor_temp:
        base = pathlib.Path(ancestor_temp)
        target = base / "target"
        (target / "runner").mkdir(parents=True)
        alias = base / "alias"
        alias.symlink_to(target, target_is_directory=True)
        os.environ["RUNNER_TEMP"] = str(alias / "runner")
        os.environ["GRADLE_USER_HOME"] = str(alias / "runner/velnor/native/gradle")
        try:
            engine["safe_home"]()
        except ValueError:
            pass
        else:
            raise AssertionError("runner ancestor symlink accepted")

"""Offline filesystem fixture for the Gradle artifact sanitizer."""
import hashlib
import json
import os
import pathlib
import sys
import tempfile


engine = {}
exec(sys.argv[1], engine)
jar = b"closed public jar fixture"
digest = hashlib.sha256(jar).hexdigest()
sha1 = hashlib.sha1(jar).hexdigest()
key = ("test.group", "fixture", "1.0.0")
engine["CLOSED"] = {key: digest}

with tempfile.TemporaryDirectory() as temp:
    runner = pathlib.Path(temp).resolve()
    home = runner / "velnor/native/gradle"
    artifact_root = home / "caches/modules-2/files-2.1"
    allowed = artifact_root / "test.group/fixture/1.0.0" / sha1 / "fixture-1.0.0.jar"
    allowed.parent.mkdir(parents=True)
    allowed.write_bytes(jar)
    private = artifact_root / "private/secret/1.0/private.jar"
    private.parent.mkdir(parents=True)
    private.write_bytes(b"private")
    manifest = home / "velnor-public-artifacts.json"
    manifest.parent.mkdir(parents=True, exist_ok=True)
    manifest_body = json.dumps([{
        "group": key[0], "module": key[1], "version": key[2],
        "sha256": digest, "path": str(allowed.resolve()),
    }]) + "\n"
    manifest.write_text(manifest_body)
    os.environ["RUNNER_TEMP"] = str(runner)
    os.environ["GRADLE_USER_HOME"] = str(home)
    os.environ["VELNOR_GRADLE_ARTIFACT_HOME_ROLE"] = "consumer"
    os.environ["VELNOR_GRADLE_PUBLIC_PROOF_SAFE"] = "true"
    engine["sanitize"]()
    assert allowed.read_bytes() == jar
    assert not private.exists()
    os.environ["VELNOR_GRADLE_ARTIFACT_HOME_ROLE"] = "producer"
    try:
        engine["sanitize"]()
    except ValueError:
        pass
    else:
        raise AssertionError("consumer home accepted for producer role")
    os.environ["VELNOR_GRADLE_ARTIFACT_HOME_ROLE"] = "consumer"
    os.environ["GRADLE_USER_HOME"] = str(runner / "velnor/native/wrong")
    try:
        engine["sanitize"]()
    except ValueError:
        pass
    else:
        raise AssertionError("wrong role root accepted")
    os.environ["GRADLE_USER_HOME"] = str(home)
    forged = allowed.read_bytes()
    allowed.write_bytes(b"forged")
    try:
        engine["sanitize"]()
    except ValueError:
        pass
    else:
        raise AssertionError("forged artifact accepted")
    allowed.write_bytes(forged)

    with tempfile.TemporaryDirectory() as external_temp:
        external = pathlib.Path(external_temp)
        secret = external / "secret.jar"
        secret.write_bytes(jar)
        allowed.unlink()
        allowed.symlink_to(secret)
        try:
            engine["sanitize"]()
        except ValueError:
            pass
        else:
            raise AssertionError("artifact symlink accepted")
        assert secret.read_bytes() == jar
        allowed.unlink()
        os.link(secret, allowed)
        try:
            engine["sanitize"]()
        except ValueError:
            pass
        else:
            raise AssertionError("artifact hardlink accepted")
        assert secret.read_bytes() == jar
        allowed.unlink()
        allowed.write_bytes(jar)

        external_manifest = external / "manifest.json"
        external_manifest.write_text(manifest_body)
        manifest.unlink()
        manifest.symlink_to(external_manifest)
        try:
            engine["sanitize"]()
        except ValueError:
            pass
        else:
            raise AssertionError("manifest symlink accepted")
        assert external_manifest.read_text() == manifest_body
        manifest.unlink()
        manifest.write_text(manifest_body)

    with tempfile.TemporaryDirectory() as redirected_temp:
        redirected_runner = pathlib.Path(redirected_temp).resolve()
        redirected_home = redirected_runner / "velnor/native/gradle"
        redirected_cache = redirected_home / "caches/modules-2/files-2.1"
        redirected_cache.parent.mkdir(parents=True)
        outside = redirected_runner / "outside"
        outside.mkdir()
        (outside / "sentinel").write_bytes(b"private")
        redirected_cache.symlink_to(outside, target_is_directory=True)
        redirected_manifest = redirected_home / "velnor-public-artifacts.json"
        redirected_manifest.write_text(json.dumps([{
            "group": key[0], "module": key[1], "version": key[2],
            "sha256": digest,
            "path": str(redirected_cache / "test.group/fixture/1.0.0" / sha1 / "fixture-1.0.0.jar"),
        }]) + "\n")
        os.environ["RUNNER_TEMP"] = str(redirected_runner)
        os.environ["GRADLE_USER_HOME"] = str(redirected_home)
        os.environ["VELNOR_GRADLE_ARTIFACT_HOME_ROLE"] = "consumer"
        try:
            engine["sanitize"]()
        except ValueError:
            pass
        else:
            raise AssertionError("cache ancestor symlink accepted")
        assert (outside / "sentinel").read_bytes() == b"private"

    with tempfile.TemporaryDirectory() as bounded_temp:
        bounded_runner = pathlib.Path(bounded_temp).resolve()
        bounded_home = bounded_runner / "velnor/native/gradle"
        bounded_root = bounded_home / "caches/modules-2/files-2.1"
        bounded_allowed = bounded_root / "test.group/fixture/1.0.0" / sha1 / "fixture-1.0.0.jar"
        bounded_allowed.parent.mkdir(parents=True)
        bounded_allowed.write_bytes(jar)
        bounded_manifest = bounded_home / "velnor-public-artifacts.json"
        bounded_manifest.parent.mkdir(parents=True, exist_ok=True)
        bounded_manifest.write_text(json.dumps([{
            "group": key[0], "module": key[1], "version": key[2],
            "sha256": digest, "path": str(bounded_allowed.resolve()),
        }]) + "\n")
        many = bounded_root / "private"
        many.mkdir()
        for index in range(engine["MAX_ENTRIES"] + 1):
            (many / str(index)).write_bytes(b"private")
        os.environ["RUNNER_TEMP"] = str(bounded_runner)
        os.environ["GRADLE_USER_HOME"] = str(bounded_home)
        os.environ["VELNOR_GRADLE_ARTIFACT_HOME_ROLE"] = "consumer"
        try:
            engine["sanitize"]()
        except ValueError:
            pass
        else:
            raise AssertionError("unbounded cache traversal accepted")

    with tempfile.TemporaryDirectory() as bytes_temp:
        bytes_runner = pathlib.Path(bytes_temp).resolve()
        bytes_home = bytes_runner / "velnor/native/gradle"
        bytes_root = bytes_home / "caches/modules-2/files-2.1"
        bytes_allowed = bytes_root / "test.group/fixture/1.0.0" / sha1 / "fixture-1.0.0.jar"
        bytes_allowed.parent.mkdir(parents=True)
        bytes_allowed.write_bytes(jar)
        bytes_manifest = bytes_home / "velnor-public-artifacts.json"
        bytes_manifest.parent.mkdir(parents=True, exist_ok=True)
        bytes_manifest.write_text(json.dumps([{
            "group": key[0], "module": key[1], "version": key[2],
            "sha256": digest, "path": str(bytes_allowed.resolve()),
        }]) + "\n")
        huge = bytes_root / "private.bin"
        with huge.open("wb") as output:
            output.truncate(engine["MAX_TOTAL"] + 1)
        os.environ["RUNNER_TEMP"] = str(bytes_runner)
        os.environ["GRADLE_USER_HOME"] = str(bytes_home)
        os.environ["VELNOR_GRADLE_ARTIFACT_HOME_ROLE"] = "consumer"
        try:
            engine["sanitize"]()
        except ValueError:
            pass
        else:
            raise AssertionError("unbounded cache bytes accepted")

    with tempfile.TemporaryDirectory() as producer_temp:
        producer_runner = pathlib.Path(producer_temp).resolve()
        producer_home = producer_runner / "velnor/native/gradle-producer/gradle-home"
        producer_root = producer_home / "caches/modules-2/files-2.1"
        producer_allowed = producer_root / "test.group/fixture/1.0.0" / sha1 / "fixture-1.0.0.jar"
        producer_allowed.parent.mkdir(parents=True)
        producer_allowed.write_bytes(jar)
        producer_manifest = producer_home / "velnor-public-artifacts.json"
        producer_manifest.parent.mkdir(parents=True, exist_ok=True)
        producer_manifest.write_text(json.dumps([{
            "group": key[0], "module": key[1], "version": key[2],
            "sha256": digest, "path": str(producer_allowed.resolve()),
        }]) + "\n")
        os.environ["RUNNER_TEMP"] = str(producer_runner)
        os.environ["GRADLE_USER_HOME"] = str(producer_home)
        os.environ["VELNOR_GRADLE_ARTIFACT_HOME_ROLE"] = "producer"
        engine["sanitize"]()
        os.environ["VELNOR_GRADLE_ARTIFACT_HOME_ROLE"] = "consumer"
        try:
            engine["sanitize"]()
        except ValueError:
            pass
        else:
            raise AssertionError("producer home accepted for consumer role")

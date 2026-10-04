"""Prepare the closed Gradle compiler project.

The producer never enters the consumer's Gradle root.  It proves one source
file and two anonymous public JARs, then writes a fresh project containing
only the Java plugin and that source file.
"""
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import sys


SOURCE = (
    "processor-target-validation/src/main/java/"
    "com/chainargos/processor/binding/RpcEndpointBinding.java"
)
SOURCE_SHA256 = "5860e10d5ce8657e00b38824fd8a68888bfab44397e9aaa8fd8df300a9c43ba4"
ROOT = "backend"
GRADLE_VERSION = "9.5.1"
JAVA_VERSION = "25.0.4.1"
COORDINATE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9_.+-]*$")
SHA256 = re.compile(r"^[0-9a-f]{64}$")
MAX_MANIFEST_BYTES = 64 * 1024
MAX_SOURCE_BYTES = 1024 * 1024
MAX_ARTIFACT_BYTES = 128 * 1024 * 1024
READ_CHUNK = 1024 * 1024

def _regular(info):
    return (not stat.S_ISLNK(info.st_mode) and stat.S_ISREG(info.st_mode)
            and info.st_nlink == 1)


def directory(path, create=False):
    if path.exists() or path.is_symlink():
        info = os.lstat(path)
        if stat.S_ISLNK(info.st_mode) or not stat.S_ISDIR(info.st_mode):
            raise ValueError("redirected producer directory")
    elif create:
        path.mkdir(mode=0o700, parents=False)
    else:
        raise ValueError("missing producer directory")
    return path


def _checked_open(path, maximum):
    before = os.lstat(path)
    if not _regular(before) or before.st_size > maximum:
        raise ValueError("redirected, shared, or oversized regular file")
    flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0)
    flags |= getattr(os, "O_NONBLOCK", 0)
    descriptor = os.open(path, flags)
    try:
        after = os.fstat(descriptor)
        if (not _regular(after) or after.st_dev != before.st_dev
                or after.st_ino != before.st_ino or after.st_size > maximum):
            raise ValueError("regular file identity changed")
        return descriptor, after
    except BaseException:
        os.close(descriptor)
        raise


def _stable(before, after, size):
    return (_regular(after) and after.st_dev == before.st_dev
            and after.st_ino == before.st_ino and after.st_size == size
            and after.st_mtime_ns == before.st_mtime_ns
            and after.st_ctime_ns == before.st_ctime_ns)


def read_bytes(path, maximum, expected=None):
    descriptor, before = _checked_open(path, maximum)
    value = hashlib.sha256() if expected is not None else None
    body = bytearray()
    try:
        while True:
            chunk = os.read(descriptor, READ_CHUNK)
            if not chunk:
                break
            body.extend(chunk)
            if len(body) > maximum:
                raise ValueError("regular file exceeds bound")
            if value is not None:
                value.update(chunk)
        after = os.fstat(descriptor)
        if not _stable(before, after, len(body)):
            raise ValueError("regular file changed while reading")
        if value is not None and value.hexdigest() != expected:
            raise ValueError("regular file digest")
        return bytes(body)
    finally:
        os.close(descriptor)


def digests(path, maximum):
    descriptor, before = _checked_open(path, maximum)
    sha256 = hashlib.sha256()
    sha1 = hashlib.sha1()
    size = 0
    try:
        while True:
            chunk = os.read(descriptor, READ_CHUNK)
            if not chunk:
                break
            size += len(chunk)
            if size > maximum:
                raise ValueError("artifact bytes exceed bound")
            sha256.update(chunk)
            sha1.update(chunk)
        after = os.fstat(descriptor)
        if not _stable(before, after, size):
            raise ValueError("artifact changed while reading")
        return size, sha256.hexdigest(), sha1.hexdigest()
    finally:
        os.close(descriptor)


def clean_path(path):
    if path.exists() or path.is_symlink():
        raise ValueError("producer path already exists")
    path.mkdir(mode=0o700, parents=False)
    return path


def pristine_directory(path):
    directory(path, create=True)
    return path


def ensure_directory(path):
    missing = []
    current = path
    while not current.exists() and not current.is_symlink():
        missing.append(current)
        current = current.parent
    directory(current)
    for child in reversed(missing):
        child.mkdir(mode=0o700)
        directory(child)
    return path


def artifact_allowlist(encoded):
    values = json.loads(bytes.fromhex(encoded).decode("utf-8"))
    expected = {
        "group", "module", "version", "jar_algorithm", "jar_checksum",
        "jar_sha256", "pom_algorithm", "pom_checksum",
    }
    if not isinstance(values, list) or not values:
        raise ValueError("closed artifact descriptors")
    if len(values) != 2:
        raise ValueError("closed artifact descriptor cardinality")
    artifacts = {}
    for value in values:
        if not isinstance(value, dict) or set(value) != expected:
            raise ValueError("closed artifact descriptor shape")
        coordinate = (value["group"], value["module"], value["version"])
        if (not all(isinstance(part, str) and COORDINATE.fullmatch(part)
                    for part in coordinate)
                or not isinstance(value["jar_sha256"], str)
                or not SHA256.fullmatch(value["jar_sha256"])):
            raise ValueError("closed artifact descriptor value")
        if coordinate in artifacts:
            raise ValueError("duplicate artifact descriptor")
        artifacts[coordinate] = value["jar_sha256"]
    return artifacts


def verify_manifest(home, artifacts):
    directory(home)
    for name in ("init.gradle", "init.gradle.kts", "init.d", "gradle.properties"):
        if (home / name).exists() or (home / name).is_symlink():
            raise ValueError("producer Gradle home contains executable config")
    manifest = home / "velnor-public-artifacts.json"
    values = json.loads(read_bytes(manifest, MAX_MANIFEST_BYTES))
    if not isinstance(values, list) or len(values) != len(artifacts):
        raise ValueError("closed artifact manifest cardinality")
    seen = set()
    paths = []
    artifact_bytes = 0
    cache = home / "caches" / "modules-2" / "files-2.1"
    directory(cache)
    for value in values:
        if not isinstance(value, dict) or set(value) != {"group", "module", "version", "sha256", "path"}:
            raise ValueError("closed artifact manifest shape")
        key = (value["group"], value["module"], value["version"])
        if key in seen or key not in artifacts or value["sha256"] != artifacts[key]:
            raise ValueError("closed artifact identity")
        path = Path(value["path"])
        if not path.is_absolute() or path.parent.parent.parent.parent.parent != cache:
            raise ValueError("artifact path root")
        relative = path.relative_to(cache)
        if relative.parts[:3] != key or len(relative.parts) != 5:
            raise ValueError("artifact path coordinates")
        if relative.parts[4] != key[1] + "-" + key[2] + ".jar":
            raise ValueError("artifact filename")
        if len(relative.parts[3]) != 40 or any(c not in "0123456789abcdef" for c in relative.parts[3]):
            raise ValueError("artifact digest path")
        if path.resolve() != path:
            raise ValueError("artifact bytes")
        size, sha256, sha1 = digests(path, MAX_ARTIFACT_BYTES - artifact_bytes)
        artifact_bytes += size
        if sha256 != value["sha256"] or sha1 != relative.parts[3]:
            raise ValueError("artifact bytes")
        seen.add(key)
        paths.append(path)
    if seen != set(artifacts):
        raise ValueError("incomplete artifact manifest")
    return sorted(paths)


def write_new(path, body):
    if path.exists() or path.is_symlink():
        raise ValueError("generated file already exists")
    ensure_directory(path.parent)
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0)
    descriptor = os.open(path, flags, 0o600)
    try:
        info = os.fstat(descriptor)
        if not stat.S_ISREG(info.st_mode) or info.st_nlink != 1:
            raise ValueError("generated file is redirected")
        with os.fdopen(descriptor, "w", encoding="utf-8", newline="\n") as output:
            output.write(body)
        descriptor = None
    finally:
        if descriptor is not None:
            os.close(descriptor)


def write_new_bytes(path, body):
    if path.exists() or path.is_symlink():
        raise ValueError("generated file already exists")
    ensure_directory(path.parent)
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0)
    descriptor = os.open(path, flags, 0o600)
    try:
        info = os.fstat(descriptor)
        if not stat.S_ISREG(info.st_mode) or info.st_nlink != 1:
            raise ValueError("generated file is redirected")
        with os.fdopen(descriptor, "wb") as output:
            output.write(body)
        descriptor = None
    finally:
        if descriptor is not None:
            os.close(descriptor)


def kotlin_string(value):
    return json.dumps(str(value), ensure_ascii=True)


def write_project(project, working, artifacts):
    write_new(project / "settings.gradle.kts", """rootProject.name = \"velnor-gradle-producer\"\ninclude(\":processor-target-validation\")\n""")
    write_new(project / "build.gradle.kts", "// Empty root build: the child owns the only Java plugin.\n")
    classpath = ",\n".join("        " + kotlin_string(path) for path in artifacts)
    build = f'''import org.gradle.api.tasks.compile.JavaCompile
import org.gradle.jvm.toolchain.JvmVendorSpec

plugins {{
    java
}}

java {{
    toolchain {{
        languageVersion.set(JavaLanguageVersion.of(25))
        vendor.set(JvmVendorSpec.GRAAL_VM)
    }}
}}

dependencies {{
    implementation(files(
{classpath}
    ))
}}

tasks.named<JavaCompile>("compileJava") {{
    options.compilerArgs.add("-parameters")
}}
'''
    write_new(project / "processor-target-validation/build.gradle.kts", build)
    source = project / SOURCE
    ensure_directory(source.parent)
    workspace = Path(os.environ["GITHUB_WORKSPACE"])
    source_path = workspace / ROOT / SOURCE
    if source_path.resolve() != source_path:
        raise ValueError("reviewed source fingerprint")
    source_body = read_bytes(source_path, MAX_SOURCE_BYTES, SOURCE_SHA256)
    write_new_bytes(source, source_body)
    if source.resolve() != source:
        raise ValueError("copied source fingerprint")
    clean_path(project / "build")
    directory(working, create=True)


def main():
    if len(sys.argv) != 3:
        raise ValueError("producer policy and artifact arguments")
    root = os.environ.get("VELNOR_GRADLE_PRODUCER_ROOT")
    if root != ROOT:
        raise ValueError("closed producer root")
    runner = Path(os.environ["RUNNER_TEMP"])
    if not runner.is_absolute() or runner.resolve() != runner:
        raise ValueError("redirected runner temporary directory")
    directory(runner)
    owner = runner / "velnor" / "native"
    directory(runner / "velnor", create=True)
    directory(owner, create=True)
    producer = pristine_directory(owner / "gradle-producer")
    allowed_existing = {"gradle-home"}
    if any(entry.name not in allowed_existing for entry in producer.iterdir()):
        raise ValueError("producer directory contains unexpected state")
    project = producer / "project"
    project = clean_path(project)
    home = producer / "gradle-home"
    pristine_directory(home)
    state = producer / "state"
    state = clean_path(state)
    empty_home = producer / "empty-home"
    empty_home = clean_path(empty_home)
    working = producer / "native-cache-working"
    clean_path(working)
    home_env = Path(os.environ["GRADLE_USER_HOME"])
    if home_env.resolve() != home.resolve():
        raise ValueError("producer Gradle home mismatch")
    allowlist = artifact_allowlist(sys.argv[2])
    artifacts = verify_manifest(home, allowlist)
    write_project(project, working, artifacts)
    policy = producer / "producer-policy.init.gradle"
    write_new(policy, bytes.fromhex(sys.argv[1]).decode("utf-8"))
    if os.environ.get("JAVA_HOME", "") == "":
        raise ValueError("pinned Java home missing")
    if GRADLE_VERSION != "9.5.1" or JAVA_VERSION != "25.0.4.1":
        raise ValueError("producer pin drift")


if __name__ == "__main__":
    main()

"""Prove and stage the reviewed public Gradle artifacts.
The descriptor set is deliberately closed.  The runner receives coordinates
from generated Rust, but this copy of the allowlist makes an edited workflow
or ambient environment unable to turn the step into an arbitrary downloader.
Maven Central is contacted directly with no proxy, credential, or redirect.
"""
import hashlib
import http.client
import json
import os
from pathlib import Path
import re
import stat
import time
import xml.etree.ElementTree as xml
HOST = "repo.maven.apache.org"
BASE_PATH = "/maven2/"
MAX_CHECKSUM = 4096
MAX_POM = 4 * 1024 * 1024
MAX_JAR = 64 * 1024 * 1024
MAX_TOTAL = 128 * 1024 * 1024
MAX_ARTIFACTS = 8
HEX = re.compile(r"^[0-9a-f]+$")
# Maven Central currently publishes SHA-256 for Micronaut and SHA-1 for the
# SLF4J artifact.  The latter is still an official immutable Central checksum;
# its stronger SHA-256 fingerprint is fixed below and is independently checked
# after the source checksum succeeds.
CLOSED = {
    ("io.micronaut", "micronaut-core", "4.10.14"): {
        "jar_algorithm": "sha256",
        "jar_checksum": "2485a578736b3d013aecf17d5c1a4ee2669754af185d5bc6b5b12c7d685129ad",
        "jar_sha256": "2485a578736b3d013aecf17d5c1a4ee2669754af185d5bc6b5b12c7d685129ad",
        "pom_algorithm": "sha256",
        "pom_checksum": "543125333530726e051998c583a748d2bfe3642f5253b9163f1b8f6f3b7abb99",
    },
    ("org.slf4j", "slf4j-api", "2.0.17"): {
        "jar_algorithm": "sha1",
        "jar_checksum": "d9e58ac9c7779ba3bf8142aff6c830617a7fe60f",
        "jar_sha256": "7b751d952061954d5abfed7181c1f645d336091b679891591d63329c622eb832",
        "pom_algorithm": "sha1",
        "pom_checksum": "0570964f8e6716b09c354c6e334ba1a092464d85",
    },
}
CACHE_SHA1 = {
    ("io.micronaut", "micronaut-core", "4.10.14"):
        ("eefdb153e2b12d160e8ce59cf0bc1e6ba4a73b03", "78cc52dc7ac37cde1e965249aca00aad75f86257"),
    ("org.slf4j", "slf4j-api", "2.0.17"):
        ("d9e58ac9c7779ba3bf8142aff6c830617a7fe60f", "0570964f8e6716b09c354c6e334ba1a092464d85"),
}
HOME_ROLES = {
    "consumer": ("velnor", "native", "gradle"),
    "producer": ("velnor", "native", "gradle-producer", "gradle-home"),
}
class Budget:
    def __init__(self):
        self.used = 0
        self.deadline = time.monotonic() + 40
    def consume(self, size):
        self.used += size
        if self.used > MAX_TOTAL or time.monotonic() > self.deadline:
            raise ValueError("proof budget exceeded")
def coordinate_path(source, suffix):
    group = source["group"]
    module = source["module"]
    version = source["version"]
    if (group, module, version) not in CLOSED:
        raise ValueError("closed artifact set")
    if (not re.fullmatch(r"[a-z0-9][a-z0-9_.-]*", group)
            or not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.-]*", module)
            or not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.+-]*", version)):
        raise ValueError("invalid Maven coordinate")
    path = "/".join((group.replace(".", "/"), module, version,
                     module + "-" + version + suffix))
    return BASE_PATH + path
def open_public(path, deadline):
    if not path.startswith(BASE_PATH) or ".." in path.split("/"):
        raise ValueError("invalid Central path")
    remaining = deadline - time.monotonic()
    if remaining <= 0:
        raise TimeoutError("proof deadline")
    connection = http.client.HTTPSConnection(
        HOST, timeout=min(5, remaining), context=None)
    try:
        # http.client reads no Maven settings, token env, netrc, cookies, or
        # proxy env.  A redirect is rejected by status handling below.
        connection.request("GET", path, headers={
            "Accept": "text/plain, application/xml, application/java-archive",
            "Accept-Encoding": "identity",
            "User-Agent": "velnor-gradle-public-proof-v1",
        })
        response = connection.getresponse()
        if response.status != 200 or response.getheader("Content-Encoding") not in (None, "identity"):
            raise ValueError("public retrieval unavailable")
        return connection, response
    except Exception:
        connection.close()
        raise
def fetch(path, maximum, budget, transport=open_public):
    connection, response = transport(path, budget.deadline)
    chunks = []
    size = 0
    try:
        declared = response.getheader("Content-Length")
        if declared is not None and (not declared.isdigit() or int(declared) > maximum):
            raise ValueError("source exceeds proof bound")
        while True:
            chunk = response.read(min(65536, maximum + 1 - size))
            if not chunk:
                break
            size += len(chunk)
            budget.consume(len(chunk))
            if size > maximum:
                raise ValueError("source exceeds proof bound")
            chunks.append(chunk)
        return b"".join(chunks)
    finally:
        connection.close()
def checksum_value(body, length):
    value = body.decode("ascii").strip()
    if len(value) != length or not HEX.fullmatch(value):
        raise ValueError("malformed Central checksum")
    return value
def source_identity(source, pom):
    if b"<!DOCTYPE" in pom or b"<!ENTITY" in pom:
        raise ValueError("unsafe POM declarations")
    root = xml.fromstring(pom)
    local = lambda tag: tag.rsplit("}", 1)[-1]
    if local(root.tag) != "project":
        raise ValueError("invalid POM root")
    def child(parent, name):
        values = [item.text.strip() for item in parent
                  if local(item.tag) == name and item.text and item.text.strip()]
        if len(values) != 1:
            raise ValueError("ambiguous POM coordinate")
        return values[0]
    artifact = child(root, "artifactId")
    group = next((child(root, "groupId") for item in root
                  if local(item.tag) == "groupId"), None)
    version = next((child(root, "version") for item in root
                    if local(item.tag) == "version"), None)
    if group is None or version is None:
        parents = [item for item in root if local(item.tag) == "parent"]
        if len(parents) != 1:
            raise ValueError("POM parent missing")
        parent = parents[0]
        group = group or child(parent, "groupId")
        version = version or child(parent, "version")
    expected = (source["group"], source["module"], source["version"])
    if (group, artifact, version) != expected:
        raise ValueError("POM coordinate mismatch")
def reject_links(path):
    if not path.is_absolute() or any(part in (".", "..") for part in path.parts):
        raise ValueError("unresolved Gradle path")
    current = Path(path.anchor)
    for component in path.parts[1:]:
        current /= component
        if current.is_symlink():
            raise ValueError("redirected Gradle path")
def read_bounded(path, maximum, budget):
    descriptor = os.open(path, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0))
    source = None
    try:
        info = os.fstat(descriptor)
        if (not stat.S_ISREG(info.st_mode) or info.st_nlink != 1
                or info.st_size > maximum):
            raise ValueError("cached artifact exceeds proof bound")
        source = os.fdopen(descriptor, "rb")
        descriptor = None
        chunks = []
        size = 0
        while True:
            chunk = source.read(min(1024 * 1024, maximum - size + 1))
            if not chunk:
                break
            size += len(chunk)
            budget.consume(len(chunk))
            if size > maximum:
                raise ValueError("cached artifact exceeds proof bound")
            chunks.append(chunk)
        return b"".join(chunks)
    finally:
        if source is not None:
            source.close()
        if descriptor is not None:
            os.close(descriptor)
def cached_artifact(home, source, suffix, algorithm, checksum, maximum, budget):
    version_dir = home
    for component in (
        "caches", "modules-2", "files-2.1", source["group"],
        source["module"], source["version"],
    ):
        version_dir = version_dir / component
        if not version_dir.exists():
            return None
        if version_dir.is_symlink() or not version_dir.is_dir():
            raise ValueError("redirected Gradle artifact directory")
    digests = CACHE_SHA1.get((source["group"], source["module"], source["version"]))
    if digests is None:
        raise ValueError("closed cache digest missing")
    digest = digests[0 if suffix == ".jar" else 1]
    directory = version_dir / digest
    if not directory.exists():
        return None
    if directory.is_symlink() or not directory.is_dir():
        raise ValueError("redirected Gradle artifact directory")
    path = directory / (source["module"] + "-" + source["version"] + suffix)
    if path.is_symlink() or not path.exists():
        if path.is_symlink():
            raise ValueError("redirected Gradle artifact")
        return None
    if not path.is_file() or os.stat(path, follow_symlinks=False).st_nlink != 1:
        raise ValueError("redirected Gradle artifact")
    body = read_bounded(path, maximum, budget)
    if hashlib.new(algorithm, body).hexdigest() == checksum:
        return body
    if path.is_symlink() or os.stat(path, follow_symlinks=False).st_nlink != 1:
        raise ValueError("redirected Gradle artifact")
    path.unlink()
    return None
def qualify(source, budget, home, transport=open_public):
    key = (source.get("group"), source.get("module"), source.get("version"))
    expected = CLOSED.get(key)
    if expected is None or set(source) != {
        "group", "module", "version", "jar_algorithm", "jar_checksum",
        "jar_sha256", "pom_algorithm", "pom_checksum",
    }:
        return None
    if any(source[name] != expected[name] for name in expected):
        return None
    try:
        jar_path = coordinate_path(source, ".jar")
        pom_path = coordinate_path(source, ".pom")
        jar_checksum_path = coordinate_path(source, ".jar." + source["jar_algorithm"])
        pom_checksum_path = coordinate_path(source, ".pom." + source["pom_algorithm"])
        jar_digest = fetch(jar_checksum_path, MAX_CHECKSUM, budget, transport)
        pom_digest = fetch(pom_checksum_path, MAX_CHECKSUM, budget, transport)
        jar_algorithm = source["jar_algorithm"]
        pom_algorithm = source["pom_algorithm"]
        if checksum_value(jar_digest, 64 if jar_algorithm == "sha256" else 40) != source["jar_checksum"]:
            return None
        if checksum_value(pom_digest, 64 if pom_algorithm == "sha256" else 40) != source["pom_checksum"]:
            return None
        jar = cached_artifact(home, source, ".jar", jar_algorithm,
                              source["jar_checksum"], MAX_JAR, budget)
        pom = cached_artifact(
            home, source, ".pom", pom_algorithm, source["pom_checksum"],
            MAX_POM, budget
        )
        if jar is None:
            jar = fetch(jar_path, MAX_JAR, budget, transport)
        if pom is None:
            pom = fetch(pom_path, MAX_POM, budget, transport)
        if hashlib.new(jar_algorithm, jar).hexdigest() != source["jar_checksum"]:
            return None
        if hashlib.new(pom_algorithm, pom).hexdigest() != source["pom_checksum"]:
            return None
        if hashlib.sha256(jar).hexdigest() != source["jar_sha256"]:
            return None
        source_identity(source, pom)
        return jar, pom
    except Exception:
        return None
def safe_home():
    value = os.environ.get("GRADLE_USER_HOME", "")
    runner_value = os.environ.get("RUNNER_TEMP", "")
    role = os.environ.get("VELNOR_GRADLE_ARTIFACT_HOME_ROLE", "")
    components = HOME_ROLES.get(role)
    if components is None:
        raise ValueError("invalid Gradle artifact home role")
    home = Path(value)
    runner = Path(runner_value)
    expected = runner.joinpath(*components)
    reject_links(runner)
    reject_links(home)
    if not runner.is_dir() or home != expected:
        raise ValueError("invalid Gradle user home")
    current = runner
    for component in components:
        child = current / component
        if child.exists() or child.is_symlink():
            if child.is_symlink() or not child.is_dir():
                raise ValueError("redirected Gradle user home")
        else:
            child.mkdir(mode=0o700)
        current = child
    return current.resolve()
def safe_child(parent, name, directory):
    child = parent / name
    if child.exists() or child.is_symlink():
        if child.is_symlink() or (directory and not child.is_dir()) or (not directory and not child.is_file()):
            raise ValueError("redirected Gradle cache path")
        if not directory and os.stat(child, follow_symlinks=False).st_nlink != 1:
            raise ValueError("shared Gradle cache file")
        return child
    if not directory:
        return child
    child.mkdir(mode=0o700)
    return child
def cache_path(home, source, digest):
    current = home
    for component in (
        "caches", "modules-2", "files-2.1", source["group"],
        source["module"], source["version"], digest,
    ):
        current = safe_child(current, component, True)
    return safe_child(current, source["module"] + "-" + source["version"] + ".jar", False)
def replace_verified(path, body):
    temporary = path.with_name("." + path.name + ".velnor-staged")
    if temporary.exists() or temporary.is_symlink():
        raise ValueError("stale staged artifact")
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0)
    descriptor = os.open(temporary, flags, 0o600)
    try:
        with os.fdopen(descriptor, "wb") as output:
            output.write(body)
        descriptor = None
        if (path.is_symlink() or not path.is_file()
                or os.stat(path, follow_symlinks=False).st_nlink != 1):
            raise ValueError("redirected existing artifact")
        os.replace(temporary, path)
    finally:
        if descriptor is not None:
            os.close(descriptor)
        if temporary.exists() or temporary.is_symlink():
            temporary.unlink()

def write_verified(path, body, maximum, budget):
    if path.exists() or path.is_symlink():
        if path.is_symlink() or not path.is_file() or os.stat(path, follow_symlinks=False).st_nlink != 1:
            raise ValueError("redirected existing artifact")
        if read_bounded(path, maximum, budget) != body:
            replace_verified(path, body)
        return
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0)
    descriptor = os.open(path, flags, 0o600)
    try:
        with os.fdopen(descriptor, "wb") as output:
            output.write(body)
        descriptor = None
    finally:
        if descriptor is not None:
            os.close(descriptor)

def write_manifest(home, entries):
    target = home / "velnor-public-artifacts.json"
    if (target.is_symlink() or (target.exists() and not target.is_file())
            or (target.exists() and os.stat(target, follow_symlinks=False).st_nlink != 1)):
        raise ValueError("redirected artifact manifest")
    payload = json.dumps(entries, sort_keys=True, separators=(",", ":")) + "\n"
    flags = os.O_WRONLY | os.O_CREAT | os.O_TRUNC | getattr(os, "O_NOFOLLOW", 0)
    descriptor = os.open(target, flags, 0o600)
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8") as output:
            output.write(payload)
        descriptor = None
    finally:
        if descriptor is not None:
            os.close(descriptor)

def qualify_sources(sources, transport=open_public):
    if not isinstance(sources, list) or len(sources) != len(CLOSED) or len(sources) > MAX_ARTIFACTS:
        raise ValueError("invalid closed artifact set")
    expected_keys = set(CLOSED)
    if {tuple(item.get(name) for name in ("group", "module", "version")) for item in sources} != expected_keys:
        raise ValueError("unexpected artifact identity")
    budget = Budget()
    home = safe_home()
    entries = []
    for source in sorted(sources, key=lambda item: (item["group"], item["module"], item["version"])):
        qualified = qualify(source, budget, home, transport)
        if qualified is None:
            raise ValueError("public artifact proof failed")
        jar, pom = qualified
        jar_sha1 = hashlib.sha1(jar).hexdigest()
        jar_path = cache_path(home, source, jar_sha1)
        write_verified(jar_path, jar, MAX_JAR, budget)
        pom_sha1 = hashlib.sha1(pom).hexdigest()
        pom_path = cache_path(home, source, pom_sha1).with_name(source["module"] + "-" + source["version"] + ".pom")
        write_verified(pom_path, pom, MAX_POM, budget)
        entries.append({
            "group": source["group"],
            "module": source["module"],
            "version": source["version"],
            "sha256": source["jar_sha256"],
            "path": str(jar_path.resolve()),
        })
    write_manifest(home, entries)
    return entries

def main():
    raw = os.environ["VELNOR_GRADLE_ARTIFACT_CANDIDATES"]
    sources = json.loads(raw)
    qualify_sources(sources)
    manifest = safe_home() / "velnor-public-artifacts.json"
    with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
        output.write("proof-safe=true\n")
        output.write("manifest-path=" + str(manifest) + "\n")

if __name__ == "__main__":
    main()

"""Deterministic pool, indexes, signatures and coherence records."""
import gzip
import os
import re
import shutil
import subprocess
import tempfile
from datetime import datetime, timezone
from email.utils import format_datetime
from pathlib import Path

from delivery_apt_core import digest, read_json, regular, require, run, write_json
from delivery_apt_stage_feed import ARCHES, PUBLICATION_SCHEMA, names, pool_path, stanzas
from delivery_apt_verify import signer


def binary_run(argv, data=None, cwd=None):
    result = subprocess.run(argv, input=data, cwd=cwd, stdout=subprocess.PIPE,
                            stderr=subprocess.PIPE, check=False)
    require(result.returncode == 0, argv[0] + " failed")
    return result.stdout


def version_order(left, right, suite):
    pattern = (r"[0-9]+\.[0-9]+\.[0-9]+" if suite == "stable" else
               r"[0-9]+\.[0-9]+\.[0-9]+~preview\.[0-9]+\+[0-9a-f]{7}")
    require(re.fullmatch(pattern, left) and re.fullmatch(pattern, right), "invalid ordered version")
    if suite == "stable":
        first = tuple(int(item) for item in left.split("."))
        second = tuple(int(item) for item in right.split("."))
        return (first > second) - (first < second)
    for operation, order in (("lt", -1), ("eq", 0), ("gt", 1)):
        result = subprocess.run(["dpkg", "--compare-versions", left, operation, right],
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False)
        require(result.returncode in (0, 1), "dpkg version comparison failed")
        if result.returncode == 0:
            return order
    raise RuntimeError("versions are not comparable")


def package_metadata(path, config, version, arch):
    regular(path)
    for field, expected in (("Package", config["package"]), ("Version", version),
                            ("Architecture", arch)):
        require(run(["dpkg-deb", "--field", str(path), field]).strip() == expected,
                "staged package metadata mismatch: " + field)


def rollback(config, verified, live):
    suite = verified["suite"]
    candidate = verified["debian_version"]
    if live is None:
        require(suite == "preview", "stable suite requires a signed rollback pair")
        return None, None
    published = live["record"]
    current = published["crate_version"]
    order = version_order(candidate, current, suite)
    require(order >= 0, "candidate would roll back live feed")
    if order == 0:
        require(published["source_record_sha256"] == verified["source_record_sha256"],
                "published candidate differs from immutable source")
        for item in verified["packages"]:
            entry = next(entry for entry in live["entries"][item["arch"]]
                         if entry["Version"] == candidate)
            require(entry["SHA256"] == item["sha256"], "published candidate package differs")
        previous_versions = {entry["Version"] for entry in live["entries"]["amd64"]
                             if entry["Version"] != candidate}
        if not previous_versions:
            require(suite == "preview" and published["previous"] is None,
                    "missing retained rollback")
            return None, None
        require(len(previous_versions) == 1, "ambiguous retained rollback")
        previous_version = previous_versions.pop()
        pointer = published["previous"]
    else:
        previous_version = current
        pointer = ("preview" if suite == "preview" else
                   {"tag": "v" + current, "source_record_sha256": published["source_record_sha256"]})
    require(version_order(candidate, previous_version, suite) > 0, "rollback pair is not older")
    if suite == "stable":
        require(isinstance(pointer, dict) and set(pointer) == {"tag", "source_record_sha256"}
                and pointer["tag"] == "v" + previous_version
                and re.fullmatch(r"[0-9a-f]{64}", pointer["source_record_sha256"]),
                "invalid stable previous pointer")
    else:
        require(pointer == "preview", "invalid preview previous pointer")
    return previous_version, pointer


def stage_pool(config, verified, live, previous_version, root):
    suite = verified["suite"]
    versions = {verified["debian_version"]}
    if previous_version is not None:
        versions.add(previous_version)
    for arch in ARCHES:
        candidate = next(item for item in verified["packages"] if item["arch"] == arch)
        source = Path("incoming") / candidate["name"]
        package_metadata(source, config, verified["debian_version"], arch)
        destination = root / pool_path(config, suite, verified["debian_version"], arch)
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_bytes(source.read_bytes())
        require(digest(destination) == candidate["sha256"], "candidate copy corrupted")
        if previous_version is not None:
            entry = next(item for item in live["entries"][arch] if item["Version"] == previous_version)
            destination = root / entry["Filename"]
            destination.write_bytes(live["files"][entry["Filename"]])
            package_metadata(destination, config, previous_version, arch)
            require(digest(destination) == entry["SHA256"], "rollback copy corrupted")
    prefix = root / ("pool/preview" if suite == "preview" else "pool/main")
    require(len(list(prefix.rglob("*.deb"))) == len(versions) * 2, "pool retention mismatch")
    return versions


def build_indexes(config, verified, versions, root):
    suite = verified["suite"]
    for arch in ARCHES:
        relative = f"dists/{suite}/main/binary-{arch}/Packages"
        path = root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        paragraphs = []
        for version in sorted(versions):
            filename = pool_path(config, suite, version, arch)
            package = root / filename
            control = run(["dpkg-deb", "--field", str(package)]).strip()
            entries = stanzas(control)
            require(len(entries) == 1, "package control must be one stanza")
            fields = entries[0]
            require(fields.get("Package") == config["package"] and fields.get("Version") == version
                    and fields.get("Architecture") == arch, "generated index identity mismatch")
            require(not {"Filename", "Size", "SHA256"}.intersection(fields),
                    "package control contains reserved index fields")
            paragraphs.append(control + f"\nFilename: {filename}\nSize: {package.stat().st_size}"
                              + "\nSHA256: " + digest(package) + "\n")
        data = ("\n".join(paragraphs) + "\n").encode("utf-8")
        path.write_bytes(data)
        (root / (relative + ".gz")).write_bytes(gzip.compress(data, compresslevel=9, mtime=0))


def signing_key(config, home):
    material = os.environ.get("APT_GPG_PRIVATE_KEY", "")
    passphrase = os.environ.get("APT_GPG_PASSPHRASE", "")
    require(material and passphrase, "APT signing secrets missing")
    os.chmod(home, 0o700)
    binary_run(["gpg", "--batch", "--homedir", home, "--import"], material.encode())
    listing = binary_run(["gpg", "--batch", "--homedir", home,
                          "--with-colons", "--list-secret-keys"]).decode()
    fingerprints = [line.split(":")[9] for line in listing.splitlines() if line.startswith("fpr:")]
    require(fingerprints and fingerprints[0] == config["signer_fingerprint"], "private key fingerprint mismatch")
    return passphrase


def sign(config, home, passphrase, root, source, destination, clear=False, armor=False):
    argv = ["gpg", "--batch", "--homedir", home, "--yes", "--pinentry-mode", "loopback",
            "--passphrase-fd", "0", "--local-user", config["signer_fingerprint"]]
    if armor:
        argv.append("--armor")
    argv += ["--output", destination, "--clearsign" if clear else "--detach-sign", source]
    binary_run(argv, passphrase.encode(), root)


def release_document(config, suite, root):
    description = config["description"] + (" (preview suite)" if suite == "preview" else "")
    fields = (("Origin", config["origin"]), ("Label", config["origin"]), ("Suite", suite),
              ("Codename", suite), ("Date", format_datetime(datetime.now(timezone.utc), usegmt=True)),
              ("Architectures", "amd64 arm64"), ("Components", "main"), ("Description", description))
    require(all("\n" not in value and "\r" not in value for field, value in fields), "unsafe Release metadata")
    lines = [field + ": " + value for field, value in fields]
    lines.append("SHA256:")
    for arch in ARCHES:
        for suffix in ("", ".gz"):
            relative = "main/binary-" + arch + "/Packages" + suffix
            path = root / "dists" / suite / relative
            regular(path)
            lines.append(f" {digest(path)} {path.stat().st_size} {relative}")
    return ("\n".join(lines) + "\n").encode("utf-8")


def sign_release(config, verified, root, home, passphrase):
    suite = verified["suite"]
    release = "dists/" + suite + "/Release"
    (root / release).write_bytes(release_document(config, suite, root))
    sign(config, home, passphrase, root, release, release + ".gpg", armor=True)
    sign(config, home, passphrase, root, release, "dists/" + suite + "/InRelease", clear=True)


def emit_records(config, verified, previous, root, home, passphrase):
    suite = verified["suite"]
    record_name, pointer_name, state_name = names(suite)
    record = {"schema": PUBLICATION_SCHEMA, "source_record_sha256": verified["source_record_sha256"],
              "tag": verified["version"] if suite == "stable" else "preview",
              "crate_version": verified["debian_version"], "previous": previous,
              "inrelease_sha256": digest(root / f"dists/{suite}/InRelease"),
              "signer_fingerprint": config["signer_fingerprint"],
              "packages": [{"arch": arch, "sha256": digest(root / f"dists/{suite}/main/binary-{arch}/Packages")}
                           for arch in ARCHES]}
    if suite == "preview":
        record["suite"] = "preview"
    write_json(root / record_name, record)
    sign(config, home, passphrase, root, record_name, record_name + ".sig")
    (root / pointer_name).write_text(verified["version"] + "\n", encoding="utf-8")
    state = {"schema": "velnor.apt-package-state.v1", "source_repository": config["source_repository"],
             "source_ref": verified["source_ref"], "source_commit": verified["commit"],
             "version": verified["version"], "packages": sorted(
                 [{"name": item["name"], "sha256": item["sha256"]} for item in verified["packages"]],
                 key=lambda item: item["name"])}
    write_json(root / state_name, state)


def publish(config, verified, live, other):
    previous_version, previous = rollback(config, verified, live)
    root = Path("public")
    require(not root.is_symlink(), "staging root is a symlink")
    if root.exists():
        shutil.rmtree(root)
    root.mkdir()
    if other is not None:
        for name, data in other["files"].items():
            destination = root / name
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_bytes(data)
    keyring = Path(config["keyring"])
    regular(keyring)
    shutil.copyfile(keyring, root / keyring.name)
    signer(dict(config, keyring=str(root / keyring.name)))
    versions = stage_pool(config, verified, live, previous_version, root)
    build_indexes(config, verified, versions, root)
    (root / "conf").mkdir()
    suites = ("stable", "preview") if other is not None else (verified["suite"],)
    distributions = "".join(f"Origin: {config['origin']}\nLabel: {config['origin']}\nCodename: {suite}\n"
                            f"Architectures: amd64 arm64\nComponents: main\nDescription: {config['description']}\n"
                            f"SignWith: {config['signer_fingerprint']}\n\n" for suite in suites)
    (root / "conf/distributions").write_text(distributions, encoding="utf-8")
    with tempfile.TemporaryDirectory(prefix="velnor-apt-sign-") as home:
        try:
            passphrase = signing_key(config, home)
            sign(config, home, passphrase, root, "/dev/null", "/dev/null")
            sign_release(config, verified, root, home, passphrase)
            emit_records(config, verified, previous, root, home, passphrase)
        finally:
            result = subprocess.run(["gpgconf", "--homedir", home, "--kill", "gpg-agent"],
                                    stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False)
            require(result.returncode == 0, "could not terminate signing agent")
    return root

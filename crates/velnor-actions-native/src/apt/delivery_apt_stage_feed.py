"""Authenticated live-feed recovery for the generated APT adapter."""
import hashlib
import re
import subprocess
import tempfile
import urllib.error
import urllib.request
from pathlib import Path, PurePosixPath

from delivery_apt_core import deb_payload, digest, elf_identity, loads, read_bytes, regular, require, run

ARCHES = ("amd64", "arm64")
PUBLICATION_SCHEMA = "velnor.publication-record/v1"


def names(suite):
    suffix = "-preview" if suite == "preview" else ""
    return ("publication-record" + suffix + ".json",
            "last-publish" + suffix, "package-state" + suffix + ".json")


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, fp, code, msg, headers, newurl):
        raise RuntimeError("APT feed redirect refused")


def fetch(config, relative, absent=False):
    path = PurePosixPath(relative)
    require(not path.is_absolute() and ".." not in path.parts and
            re.fullmatch(r"[A-Za-z0-9_./+~:-]+", relative), "unsafe feed path")
    url = config["feed_url"].rstrip("/") + "/" + relative
    require(url.startswith("https://"), "feed requires HTTPS")
    request = urllib.request.Request(url, headers={"Cache-Control": "no-cache"})
    try:
        with urllib.request.build_opener(NoRedirect()).open(request, timeout=30) as response:
            require(response.status == 200, "unexpected feed response")
            return response.read()
    except urllib.error.HTTPError as error:
        error.close()
        if absent and error.code == 404:
            return None
        raise RuntimeError("APT feed HTTP failure: " + str(error.code)) from error
    except (urllib.error.URLError, TimeoutError) as error:
        raise RuntimeError("APT feed unavailable") from error


def checked_fetch(config, relative, checksum):
    require(re.fullmatch(r"[0-9a-f]{64}", checksum), "invalid feed digest")
    data = fetch(config, relative)
    require(hashlib.sha256(data).hexdigest() == checksum, "live feed digest mismatch")
    return data


def verify_signature(config, document, signature=None):
    with tempfile.TemporaryDirectory(prefix="velnor-apt-verify-") as temporary:
        root = Path(temporary)
        content = root / "document"
        content.write_bytes(document)
        argv = ["gpgv", "--status-fd", "1", "--keyring", str(Path(config["keyring"]).resolve())]
        if signature is not None:
            detached = root / "signature"
            detached.write_bytes(signature)
            argv += [str(detached)]
        result = run(argv + [str(content)])
        fingerprints = [line.split()[11] if len(line.split()) == 12 else line.split()[2]
                        for line in result.splitlines()
                        if line.startswith("[GNUPG:] VALIDSIG ")]
        require(fingerprints == [config["signer_fingerprint"]], "feed signer disagrees")


def cleartext(inrelease):
    text = inrelease.decode("utf-8")
    require(text.startswith("-----BEGIN PGP SIGNED MESSAGE-----\n"), "invalid InRelease")
    head, separator, body = text.partition("\n\n")
    require(separator and "Hash:" in head, "invalid signed Release header")
    release, separator, signature = body.partition("-----BEGIN PGP SIGNATURE-----")
    require(separator and signature, "missing Release signature")
    return "\n".join(line[2:] if line.startswith("- ") else line
                     for line in release.splitlines()) + "\n"


def release_hashes(text, suite, config):
    fields = {}
    hashes = {}
    section = None
    for line in text.splitlines():
        if line.startswith(" "):
            if section == "SHA256":
                parts = line.split()
                require(len(parts) == 3, "invalid Release checksum")
                checksum, size, path = parts
                require(path not in hashes and size.isdigit(), "duplicate Release path")
                hashes[path] = checksum
        else:
            key, separator, value = line.partition(":")
            require(separator and key not in fields, "invalid Release field")
            fields[key] = value.strip()
            section = key
    for key, expected in (("Origin", config["origin"]), ("Label", config["origin"]),
                          ("Suite", suite), ("Codename", suite), ("Components", "main"),
                          ("Architectures", "amd64 arm64")):
        require(fields.get(key) == expected, "signed Release identity mismatch: " + key)
    require(hashes, "Release lacks SHA256 checksums")
    return hashes


def stanzas(text):
    entries = []
    for block in text.strip().split("\n\n"):
        values = {}
        for line in block.splitlines():
            if line.startswith(" "):
                continue
            key, separator, value = line.partition(":")
            require(separator and key not in values, "invalid Packages field")
            values[key] = value.strip()
        if values:
            entries.append(values)
    return entries


def pool_path(config, suite, version, arch):
    package = config["package"]
    require(re.fullmatch(r"[0-9A-Za-z.+:~_-]+", version), "unsafe package version")
    letter = package[:4] if package.startswith("lib") and len(package) >= 4 else package[0]
    prefix = "pool/preview" if suite == "preview" else "pool"
    return f"{prefix}/main/{letter}/{package}/{package}_{version}_{arch}.deb"


def validate_entries(config, suite, arch, entries):
    require(1 <= len(entries) <= 2, "live index retention mismatch")
    versions = set()
    for entry in entries:
        require(entry.get("Package") == config["package"] and
                entry.get("Architecture") == arch, "live package identity mismatch")
        version = entry.get("Version", "")
        require(version not in versions, "duplicate live package version")
        versions.add(version)
        require(entry.get("Filename") == pool_path(config, suite, version, arch),
                "noncanonical live pool path")
        require(re.fullmatch(r"[0-9a-f]{64}", entry.get("SHA256", "")), "invalid package digest")
    return versions


def publication(config, suite, absent=False):
    record_name, pointer_name, state_name = names(suite)
    raw = fetch(config, record_name, absent)
    if raw is None:
        require(fetch(config, pointer_name, True) is None and
                fetch(config, "dists/" + suite + "/InRelease", True) is None,
                "partial live suite cannot bootstrap")
        return None
    signature = fetch(config, record_name + ".sig")
    verify_signature(config, raw, signature)
    record = loads(raw.decode("utf-8"))
    require(record.get("schema") == PUBLICATION_SCHEMA, "unsupported publication schema")
    require(record.get("signer_fingerprint") == config["signer_fingerprint"], "publication signer mismatch")
    require(re.fullmatch(r"[0-9a-f]{64}", record.get("source_record_sha256", "")), "invalid source digest")
    require(record.get("suite") == ("preview" if suite == "preview" else None), "publication suite mismatch")
    tag = record.get("tag", "")
    version = record.get("crate_version", "")
    require(tag == ("preview" if suite == "preview" else "v" + version), "publication tag mismatch")
    pointer = fetch(config, pointer_name).decode("utf-8").strip()
    require(pointer == (version if suite == "preview" else tag), "live pointer disagrees with record")
    files = {record_name: raw, pointer_name: (pointer + "\n").encode(), record_name + ".sig": signature}
    return {"record": record, "files": files}


def live_suite(config, suite, absent=False):
    live = publication(config, suite, absent)
    if live is None:
        return None
    record = live["record"]
    files = live["files"]
    relative = "dists/" + suite + "/"
    inrelease = checked_fetch(config, relative + "InRelease", record["inrelease_sha256"])
    verify_signature(config, inrelease)
    release = cleartext(inrelease)
    hashes = release_hashes(release, suite, config)
    files[relative + "InRelease"] = inrelease
    files[relative + "Release"] = fetch(config, relative + "Release")
    require(files[relative + "Release"] == release.encode(), "Release differs from InRelease")
    files[relative + "Release.gpg"] = fetch(config, relative + "Release.gpg")
    verify_signature(config, files[relative + "Release"], files[relative + "Release.gpg"])
    indexes = record.get("packages", [])
    require(len(indexes) == 2 and {item.get("arch") for item in indexes} == set(ARCHES),
            "publication architectures mismatch")
    versions = None
    live["entries"] = {}
    for arch in ARCHES:
        path = "main/binary-" + arch + "/Packages"
        checksum = next(item["sha256"] for item in indexes if item["arch"] == arch)
        require(hashes.get(path) == checksum, "publication index digest mismatch")
        data = checked_fetch(config, relative + path, checksum)
        entries = stanzas(data.decode("utf-8"))
        arch_versions = validate_entries(config, suite, arch, entries)
        require(versions is None or versions == arch_versions, "live architecture versions differ")
        versions = arch_versions
        live["entries"][arch] = entries
        files[relative + path] = data
        compressed = path + ".gz"
        require(compressed in hashes, "missing signed compressed index")
        files[relative + compressed] = checked_fetch(config, relative + compressed, hashes[compressed])
        for entry in entries:
            files[entry["Filename"]] = checked_fetch(config, entry["Filename"], entry["SHA256"])
    require(record["crate_version"] in versions, "live publication candidate absent")
    state = names(suite)[2]
    state_raw = fetch(config, state)
    check_state(config, suite, record, loads(state_raw.decode()), live["entries"], files)
    files[state] = state_raw
    return live


def check_state(config, suite, record, state, entries, files):
    require(state.get("schema") == "velnor.apt-package-state.v1" and
            state.get("source_repository") == config["source_repository"], "live channel identity mismatch")
    version = record["crate_version"]
    expected_version = "v" + version if suite == "stable" else version
    expected_ref = "refs/tags/" + expected_version if suite == "stable" else "refs/heads/main"
    require(state.get("version") == expected_version and state.get("source_ref") == expected_ref,
            "live channel version mismatch")
    require(re.fullmatch(r"[0-9a-f]{40}", state.get("source_commit", "")), "invalid live channel commit")
    packages = []
    for arch in ARCHES:
        entry = next(item for item in entries[arch] if item["Version"] == version)
        asset = (f"{config['package']}-{version}-{arch}.deb" if suite == "stable" else
                 f"{config['package']}-preview-{version.replace('~', '.')}-{arch}.deb")
        packages.append({"name": asset, "sha256": entry["SHA256"]})
        check_package_state(config, suite, state, version, arch, entry, files[entry["Filename"]])
    require(state.get("packages") == sorted(packages, key=lambda item: item["name"]),
            "live channel packages mismatch")


def local_suite(config, suite, root):
    record_name, pointer_name, state_name = names(suite)
    record = loads(regular(root / record_name).read_text())
    verify_signature(config, (root / record_name).read_bytes(), regular(root / (record_name + ".sig")).read_bytes())
    require(record.get("schema") == PUBLICATION_SCHEMA and
            record.get("signer_fingerprint") == config["signer_fingerprint"], "staged publication identity mismatch")
    relative = "dists/" + suite + "/"
    inrelease = regular(root / (relative + "InRelease")).read_bytes()
    require(digest(root / (relative + "InRelease")) == record.get("inrelease_sha256"),
            "staged InRelease digest mismatch")
    verify_signature(config, inrelease)
    release = cleartext(inrelease)
    require(regular(root / (relative + "Release")).read_bytes() == release.encode(),
            "staged Release differs from InRelease")
    verify_signature(config, release.encode(), regular(root / (relative + "Release.gpg")).read_bytes())
    hashes = release_hashes(release, suite, config)
    indexes = record.get("packages", [])
    require(len(indexes) == 2 and {item.get("arch") for item in indexes} == set(ARCHES),
            "staged publication architectures mismatch")
    entries = {}
    versions = None
    for arch in ARCHES:
        path = "main/binary-" + arch + "/Packages"
        checksum = next(item["sha256"] for item in indexes if item["arch"] == arch)
        require(hashes.get(path) == checksum and digest(root / (relative + path)) == checksum,
                "staged index differs from signed Release")
        require(digest(root / (relative + path + ".gz")) == hashes.get(path + ".gz"),
                "staged compressed index differs from signed Release")
        entries[arch] = stanzas(regular(root / (relative + path)).read_text())
        arch_versions = validate_entries(config, suite, arch, entries[arch])
        require(versions is None or arch_versions == versions, "staged architecture versions differ")
        versions = arch_versions
        for item in entries[arch]:
            require(digest(root / item["Filename"]) == item["SHA256"], "staged pool differs from signed index")
    require(record["crate_version"] in versions, "staged candidate missing")
    pointer = regular(root / pointer_name).read_text().strip()
    require(pointer == (record["crate_version"] if suite == "preview" else record["tag"]),
            "staged pointer differs from record")
    files = {item["Filename"]: read_bytes(root / item["Filename"])
             for values in entries.values() for item in values if item["Version"] == record["crate_version"]}
    check_state(config, suite, record, loads(regular(root / state_name).read_text()), entries, files)
    return record


def check_package_state(config, suite, state, version, arch, entry, data):
    require(hashlib.sha256(data).hexdigest() == entry["SHA256"], "channel package digest mismatch")
    with tempfile.TemporaryDirectory(prefix="velnor-apt-identity-") as temporary:
        path = Path(temporary).resolve() / "candidate.deb"
        path.write_bytes(data)
        payload = deb_payload(path)
    identity_path = "usr/share/" + config["identity_directory"] + "/build-identity.json"
    require(identity_path in payload, "channel candidate lacks build identity")
    identity = loads(payload[identity_path].decode("utf-8"))
    base = version.split("~preview.")[0] if suite == "preview" else version
    require(identity.get("source_sha") == state["source_commit"] and
            identity.get("crate_version") == base, "channel source commit differs from signed package identity")
    binary_path = "usr/bin/" + config["binary"]
    require(binary_path in payload, "channel candidate lacks binary")
    elf_identity(payload[binary_path], arch)


def candidate_hashes(suite, root, version):
    result = {}
    for arch in ARCHES:
        path = root / f"dists/{suite}/main/binary-{arch}/Packages"
        entries = stanzas(read_bytes(path).decode("utf-8"))
        current = [item for item in entries if item.get("Version") == version]
        require(len(current) == 1, "candidate index entry missing or duplicated")
        result[arch] = current[0]["SHA256"]
    return result

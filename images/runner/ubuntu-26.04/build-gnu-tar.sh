#!/usr/bin/env bash
set -euo pipefail

input_dir="$(cd -- "$(dirname -- "$0")" && pwd)"
output_dir="${1:?output directory required}"
architecture="$(dpkg --print-architecture)"
machine="$(uname -m)"
target="$(python3 -c 'import json,pathlib,sys; print(json.loads(pathlib.Path(sys.argv[1]).read_text())["build"]["target"])' "$input_dir/gnu-tar-source.lock.json")"
if [[ "$architecture" != amd64 || "$machine" != x86_64 || "$target" != linux/amd64 ]]; then
  printf 'GNU tar build requires linux/amd64 on x86_64; got target=%s arch=%s machine=%s\n' \
    "$target" "$architecture" "$machine" >&2
  exit 1
fi
work_dir="$(mktemp -d "${TMPDIR:-/tmp}/velnor-gnu-tar.XXXXXX")"
bootstrap_path="$work_dir/bootstrap-bin"

cleanup() {
  rm -rf -- "$work_dir"
}
trap cleanup EXIT

mkdir -p -- "$work_dir/downloads" "$work_dir/bootstrap-bin"

python3 - "$input_dir" "$work_dir" <<'PY'
import hashlib
import json
import lzma
import pathlib
import re
import subprocess
import sys
import tarfile
import urllib.error
import urllib.request

input_dir = pathlib.Path(sys.argv[1])
work_dir = pathlib.Path(sys.argv[2])
downloads = work_dir / "downloads"
lock = json.loads((input_dir / "gnu-tar-source.lock.json").read_text())


def require(condition, message):
    if not condition:
        raise SystemExit(message)


def check_file(path, expected, label):
    data = path.read_bytes()
    require(len(data) == expected["size"], label + ": size mismatch")
    actual = hashlib.sha256(data).hexdigest()
    require(actual == expected["sha256"], label + ": SHA-256 mismatch")
    return data


def fetch(url, destination, expected, label):
    request = urllib.request.Request(url, headers={"User-Agent": "velnor-gnu-tar-build/1"})
    last_error = None
    for attempt in range(5):
        try:
            with urllib.request.urlopen(request, timeout=90) as response:
                data = response.read(expected["size"] + 1)
            require(len(data) == expected["size"], label + ": size mismatch")
            actual = hashlib.sha256(data).hexdigest()
            require(actual == expected["sha256"], label + ": SHA-256 mismatch")
            destination.write_bytes(data)
            return data
        except (urllib.error.URLError, TimeoutError) as error:
            last_error = error
            if attempt == 4:
                break
    raise SystemExit(label + ": download failed: " + str(last_error))


def fields(block):
    result = {}
    key = None
    for line in block.splitlines():
        if line.startswith((" ", "\t")):
            require(key is not None, "source metadata: stray continuation")
            result[key] += "\n" + line[1:]
            continue
        name, separator, value = line.partition(":")
        require(separator, "source metadata: malformed field")
        key = name
        result[key] = value.lstrip()
    return result


def sha256_entries(value):
    rows = {}
    for line in value.splitlines():
        if not line.strip():
            continue
        parts = line.split()
        require(len(parts) == 3, "source metadata: malformed SHA-256 entry")
        digest, size, name = parts
        require(name not in rows, "source metadata: duplicate source file")
        rows[name] = {"sha256": digest, "size": int(size)}
    return rows


authority = lock["authority"]
snapshot = authority["snapshot"]
snapshot_id = snapshot["id"]
require(re.fullmatch(r"[0-9]{8}T[0-9]{6}Z", snapshot_id) is not None,
        "The Ubuntu archive snapshot identifier is invalid.")
snapshot_base_url = "https://snapshot.ubuntu.com/ubuntu/" + snapshot_id
require(snapshot["base_url"] == snapshot_base_url,
        "The Ubuntu archive snapshot base URL is invalid.")
expected_inrelease_url = snapshot_base_url + "/dists/" + lock["distribution"] + "/InRelease"
require(authority["inrelease"]["url"] == expected_inrelease_url,
        "The InRelease URL does not match the expected URL.")
expected_sources_url = (snapshot_base_url + "/dists/" + lock["distribution"] + "/"
                       + authority["sources_index"]["path_in_release"])
require(authority["sources_index"]["url"] == expected_sources_url,
        "The Sources URL does not match the expected URL.")
keyring_path = downloads / "ubuntu-archive-keyring.gpg"
fetch(authority["archive_keyring"]["url"], keyring_path,
      authority["archive_keyring"], "Ubuntu archive keyring")
inrelease_path = downloads / "InRelease"
inrelease = fetch(authority["inrelease"]["url"], inrelease_path,
                  authority["inrelease"], "Ubuntu InRelease")
status = subprocess.run(
    ["gpgv", "--status-fd=1", "--keyring", str(keyring_path), str(inrelease_path)],
    check=True, capture_output=True, text=True,
).stdout
fingerprints = re.findall(r"^\[GNUPG:\] VALIDSIG ([0-9A-F]+)(?: |$)", status, re.M)
require(authority["signer_fingerprint"] in fingerprints,
        "Ubuntu InRelease: unexpected signer")

release_text = inrelease.decode("utf-8")
in_sha256 = False
release_rows = []
for line in release_text.splitlines():
    if line == "SHA256:":
        in_sha256 = True
        continue
    if in_sha256 and line and not line[0].isspace():
        in_sha256 = False
    if in_sha256:
        parts = line.split()
        if len(parts) == 3 and parts[2] == authority["sources_index"]["path_in_release"]:
            release_rows.append({"sha256": parts[0], "size": int(parts[1])})
require(release_rows == [{"sha256": authority["sources_index"]["sha256"],
                          "size": authority["sources_index"]["size"]}],
        "Ubuntu InRelease: Sources.xz entry mismatch")

sources_path = downloads / "Sources.xz"
sources_data = fetch(authority["sources_index"]["url"], sources_path,
                     authority["sources_index"], "Ubuntu Sources.xz")
sources_text = lzma.decompress(sources_data).decode("utf-8")
stanzas = []
for block in sources_text.strip().split("\n\n"):
    item = fields(block)
    if item.get("Package") == lock["package"] and item.get("Version") == lock["version"]:
        stanzas.append((block, item))
require(len(stanzas) == 1, "Ubuntu Sources.xz: source stanza mismatch")
stanza_text, stanza = stanzas[0]
stanza_bytes = (stanza_text.rstrip() + "\n").encode("utf-8")
require(hashlib.sha256(stanza_bytes).hexdigest() == authority["source_stanza_sha256"],
        "Ubuntu Sources.xz: signed stanza hash mismatch")
require(stanza.get("Directory") == lock["directory"],
        "Ubuntu Sources.xz: source directory mismatch")
signed_files = sha256_entries(stanza["Checksums-Sha256"])

for item in lock["source_files"]:
    signed = signed_files.get(item["name"])
    require(signed == {"sha256": item["sha256"], "size": item["size"]},
            "Ubuntu Sources.xz: source file hash mismatch for " + item["name"])
    url = snapshot_base_url + "/" + lock["directory"] + "/" + item["name"]
    fetch(url, downloads / item["name"], item, item["name"])

orig = next(item for item in lock["source_files"] if item["name"].endswith(".orig.tar.xz"))
debian = next(item for item in lock["source_files"] if item["name"].endswith(".debian.tar.xz"))
source_root = work_dir / "bootstrap"
source_root.mkdir()
with tarfile.open(downloads / orig["name"], "r:xz") as source_archive:
    members = source_archive.getmembers()
    roots = {member.name.split("/", 1)[0] for member in members}
    require(roots == {"tar-1.35+dfsg"}, "GNU tar source archive: unexpected root")
    source_archive.extractall(source_root, members=members, filter="data")
source_tree = source_root / "tar-1.35+dfsg"
with tarfile.open(downloads / debian["name"], "r:xz") as debian_archive:
    debian_archive.extractall(source_tree, filter="data")

series_path = source_tree / "debian/patches/series"
series = series_path.read_bytes()
require(hashlib.sha256(series).hexdigest() == lock["debian_patch_series_sha256"],
        "Ubuntu patch series: SHA-256 mismatch")
patch_names = []
for line in series.decode("utf-8").splitlines():
    value = line.split("#", 1)[0].strip()
    if value:
        parts = value.split()
        require(len(parts) == 1, "Ubuntu patch series: unsupported patch options")
        patch_names.append(parts[0])
expected_patches = lock["official_patches"]
require(patch_names == [patch["name"] for patch in expected_patches],
        "Ubuntu patch series: patch list mismatch")
require(set(signed_files) == {item["name"] for item in lock["source_files"]},
        "Ubuntu Sources.xz: unexpected source file closure")
for patch in expected_patches:
    path = source_tree / "debian/patches" / patch["name"]
    actual = hashlib.sha256(path.read_bytes()).hexdigest()
    require(actual == patch["sha256"], "Ubuntu patch hash mismatch: " + patch["name"])
    subprocess.run(["patch", "--batch", "-p1", "-i", str(path)],
                   cwd=source_tree, check=True)

custom = lock["custom_patch"]
require(custom["name"] == "gnu-tar-openat2-declaration.patch",
        "GNU tar correction patch: unexpected name")
custom_path = input_dir / custom["name"]
actual = hashlib.sha256(custom_path.read_bytes()).hexdigest()
require(actual == custom["sha256"], "GNU tar correction patch: SHA-256 mismatch")
subprocess.run(["patch", "--batch", "-p1", "-i", str(custom_path)],
               cwd=source_tree, check=True)
header = (source_tree / "gnu/fcntl.in.h").read_bytes()
require(hashlib.sha256(header).hexdigest() ==
        custom["result_sha256"],
        "GNU tar correction patch: resulting header mismatch")
print("Verified Ubuntu signature, source files, 28 patches, and GNU tar correction.")
PY

chown -R tarbuilder:tarbuilder "$work_dir"

# shellcheck disable=SC2016
runuser -u tarbuilder -- env WORK_DIR="$work_dir" bash -euo pipefail -c '
  cd "$WORK_DIR/bootstrap/tar-1.35+dfsg"
  export ac_cv_func_openat2=no
  export DEB_BUILD_MAINT_OPTIONS=future=+lfs
  export DEB_CFLAGS_MAINT_APPEND="-Wall -Wno-analyzer-null-argument"
  eval "$(dpkg-buildflags --export=sh)"
  aclocal -I m4
  autoconf
  autoheader
  automake --add-missing --copy
  ./configure RSH=/usr/bin/rsh --prefix=/usr --libexecdir=/usr/sbin \
    --enable-backup-scripts --with-lzma=xz --disable-silent-rules
  make -j4
  src/tar --version | grep -F "tar (GNU tar) 1.35"
'

install -D -m 0555 -- "$work_dir/bootstrap/tar-1.35+dfsg/src/tar" \
  "$bootstrap_path/tar"

# shellcheck disable=SC2016
runuser -u tarbuilder -- env WORK_DIR="$work_dir" BOOTSTRAP_PATH="$bootstrap_path" INPUT_DIR="$input_dir" \
  bash -euo pipefail -c '
    export PATH="$BOOTSTRAP_PATH:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"
    dpkg-source -x "$WORK_DIR/downloads/tar_1.35+dfsg-4ubuntu0.4.dsc" "$WORK_DIR/package"
    cd "$WORK_DIR/package"
    install -m 0644 "$INPUT_DIR/gnu-tar-openat2-declaration.patch" \
      debian/patches/gnu-tar-openat2-declaration.patch
    printf "%s\n" gnu-tar-openat2-declaration.patch >> debian/patches/series
    export ac_cv_func_openat2=no
    export DEB_BUILD_OPTIONS=parallel=4
    dpkg-buildpackage -us -uc -b
  '

python3 - "$work_dir/package/tests/testsuite.log" "$input_dir/gnu-tar-source.lock.json" <<'PY'
import json
import pathlib
import re
import sys

log = pathlib.Path(sys.argv[1]).read_text()
lock = json.loads(pathlib.Path(sys.argv[2]).read_text())

build = lock["build"]
rows = [line for line in log.splitlines() if re.match(r"^\s*\d+\.\s", line)]
results = {}
for line in rows:
    match = re.match(r"^\s*(\d+)\.\s+(.+):\s+(\S+)(?:\s+.*)?$", line)
    if match is None:
        raise SystemExit("GNU tar package tests: unrecognized result row: " + line)
    number = int(match.group(1))
    if number in results:
        raise SystemExit(f"GNU tar package tests: duplicate result for test {number}")
    status = match.group(3)
    if status not in ("ok", "skipped"):
        raise SystemExit(f"GNU tar package tests: failed result {status}: " + line)
    results[number] = status

passed_summary = re.findall(r"(?m)^(\d+) tests were successful\.$", log)
skipped_summary = re.findall(r"(?m)^(\d+) tests were skipped\.$", log)
if len(passed_summary) != 1 or len(skipped_summary) != 1:
    raise SystemExit("GNU tar package tests: missing or repeated test summary")
passed = sum(result == "ok" for result in results.values())
skipped = sum(result == "skipped" for result in results.values())
if passed != int(passed_summary[0]) or skipped != int(skipped_summary[0]):
    raise SystemExit("GNU tar package tests: result rows do not match summary counts")

for number in build["required_tests"]:
    if results.get(number) != "ok":
        raise SystemExit(f"GNU tar package tests: required test {number} did not pass")

reference = build["reference_test_results"]
print(f"GNU tar package tests: {passed} passed, {skipped} skipped, 0 failed.")
print(f"Reference run: {reference['passed']} passed, {reference['skipped']} skipped.")
for line in rows:
    if re.search(r":\s+skipped(?:\s|$)", line):
        print("GNU tar package test skipped: " + line.strip())
print("Verified required GNU tar tests: " + ", ".join(map(str, build["required_tests"])) + ".")
PY

mkdir -p -- "$output_dir/usr/bin"
package="$work_dir/tar_1.35+dfsg-4ubuntu0.4_amd64.deb"
test -f "$package"
expected_package_sha256="$(python3 -c 'import json,pathlib,sys; print(json.loads(pathlib.Path(sys.argv[1]).read_text())["runtime"]["package_sha256"])' "$input_dir/gnu-tar-source.lock.json")"
expected_tar_sha256="$(python3 -c 'import json,pathlib,sys; print(json.loads(pathlib.Path(sys.argv[1]).read_text())["runtime"]["tar_sha256"])' "$input_dir/gnu-tar-source.lock.json")"
printf '%s  %s\n' "$expected_package_sha256" "$package" | sha256sum -c -

install_root="$work_dir/package-payload"
mkdir -p -- "$install_root"
dpkg-deb --fsys-tarfile "$package" | env -u TAR_OPTIONS "$bootstrap_path/tar" -xpf - -C "$install_root"
tar_binary="$install_root/usr/bin/tar"
test -x "$tar_binary"
printf '%s  %s\n' "$expected_tar_sha256" "$tar_binary" | sha256sum -c -
ldd "$tar_binary" > "$work_dir/tar-ldd.txt"
if grep -Fq 'not found' "$work_dir/tar-ldd.txt"; then
  cat "$work_dir/tar-ldd.txt" >&2
  exit 1
fi
install -D -m 0755 -- "$tar_binary" "$output_dir/usr/bin/tar"
echo "Built verified GNU tar $("$output_dir/usr/bin/tar" --version | sed -n '1p')"

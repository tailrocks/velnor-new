#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0
"""Run the legacy candidate.py validate and `velnor-ci image verify-candidate` on the
same generated fixtures and compare verdicts and messages.

usage: parity-verify-candidate.py <velnor-ci binary>
"""
import bz2, copy, gzip, hashlib, importlib.util, io, json, lzma, os, subprocess, sys
import tarfile, tempfile, zlib
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
LEGACY = REPO / "images/job/ubuntu-26.04/candidate/candidate.py"
BINARY = sys.argv[1]
spec = importlib.util.spec_from_file_location("legacy_candidate", LEGACY)
legacy = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = legacy
spec.loader.exec_module(legacy)

STRICTER = {"layer pax size record", "layer bad checksum later header", "layer truncated header"}
LAYER = "application/vnd.oci.image.layer.v1.tar"
LAYER_GZ = LAYER + "+gzip"
SHA = "a" * 40
ENV = {
    "GITHUB_SHA": SHA, "GITHUB_REF": "refs/heads/candidate",
    "GITHUB_REPOSITORY": "tailrocks/velnor-new", "GITHUB_REPOSITORY_ID": "1",
    "GITHUB_EVENT_NAME": "push",
    "GITHUB_WORKFLOW_REF": "tailrocks/velnor-new/.github/workflows/release.yml@refs/heads/main",
    "GITHUB_WORKFLOW_SHA": "c" * 40, "GITHUB_RUN_ID": "10", "GITHUB_RUN_ATTEMPT": "2",
}


def dg(data): return "sha256:" + hashlib.sha256(data).hexdigest()
def jb(value): return json.dumps(value, separators=(",", ":")).encode()


def tar_of(members, fmt=tarfile.PAX_FORMAT, end=True):
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode="w", format=fmt) as archive:
        for name, data, *rest in members:
            info = tarfile.TarInfo(name)
            if data is None:
                info.type = tarfile.DIRTYPE
                archive.addfile(info)
                continue
            if rest:
                info.pax_headers = rest[0]
            info.size = len(data)
            archive.addfile(info, io.BytesIO(data))
    raw = buffer.getvalue()
    return raw if end else raw[:-10240] if len(raw) > 10240 else raw


def oci(layers, diff_ids=None, platform=None, config_extra=None, manifest_extra=None, layout=None, index_extra=None, entries_extra=()):
    blobs = [(media, data) for data, media in layers]
    diffs = diff_ids if diff_ids is not None else [
        dg(gzip.decompress(d) if m.endswith("+gzip") else d) for d, m in layers]
    config = {"architecture": "amd64", "os": "linux", "rootfs": {"type": "layers", "diff_ids": diffs}}
    config.update(config_extra or {})
    config_raw = jb(config)
    manifest = {
        "schemaVersion": 2, "mediaType": "application/vnd.oci.image.manifest.v1+json",
        "config": {"mediaType": "application/vnd.oci.image.config.v1+json", "digest": dg(config_raw), "size": len(config_raw)},
        "layers": [{"mediaType": m, "digest": dg(d), "size": len(d)} for d, m in layers],
    }
    manifest.update(manifest_extra or {})
    manifest_raw = jb(manifest)
    index = {"schemaVersion": 2, "manifests": [{
        "mediaType": "application/vnd.oci.image.manifest.v1+json", "digest": dg(manifest_raw),
        "size": len(manifest_raw), "platform": platform or {"architecture": "amd64", "os": "linux"}}]}
    index.update(index_extra or {})
    entries = [("oci-layout", layout if layout is not None else jb({"imageLayoutVersion": "1.0.0"})), ("index.json", jb(index))]
    seen = set()
    for data in [manifest_raw, config_raw] + [d for d, _ in layers]:
        if data not in seen:
            seen.add(data)
            entries.append(("blobs/sha256/" + hashlib.sha256(data).hexdigest(), data))
    return entries + list(entries_extra)


def gz(data, level=6, mtime=0):
    return gzip.compress(data, compresslevel=level, mtime=mtime)


def simple_tar(names=("job-image",), fmt=tarfile.PAX_FORMAT):
    return tar_of([(n, b"ok") for n in names], fmt)


BASE_TAR = simple_tar()
BASE_GZ = gz(BASE_TAR)


def base_entries():
    return oci([(BASE_GZ, LAYER_GZ)])


def sized(raw_tar, name, size_field):
    block = bytearray(raw_tar[:512])
    block[124:136] = size_field
    block[148:156] = b" " * 8
    block[148:155] = b"%06o\0" % sum(block)
    return bytes(block) + raw_tar[512:]


def corrupt(data, at, value=None):
    out = bytearray(data)
    out[at] = out[at] ^ 0xFF if value is None else value
    return bytes(out)


def cases():
    out = []
    def add(name, entries=None, raw=None, record=None, env=None, root=None, setup=None):
        out.append(dict(name=name, entries=entries, raw=raw, record=record, env=env, root=root, setup=setup))
    add("valid gzip layer", base_entries())
    add("valid raw layer", oci([(BASE_TAR, LAYER)]))
    add("valid two layers gzip+raw", oci([(BASE_GZ, LAYER_GZ), (simple_tar(("second",)), LAYER)]))
    add("valid repeated identical layer", oci([(BASE_GZ, LAYER_GZ), (BASE_GZ, LAYER_GZ)]))
    add("valid nondistributable media", oci([(BASE_TAR, "application/vnd.oci.image.layer.nondistributable.v1.tar")]))
    add("valid layer with pax long path", oci([(gz(tar_of([("d/" * 80 + "f", b"x")])), LAYER_GZ)]))
    add("valid layer ustar prefix", oci([(gz(tar_of([("d/" * 60 + "f", b"x")], tarfile.USTAR_FORMAT)), LAYER_GZ)]))
    add("valid layer gnu longname", oci([(gz(tar_of([("d/" * 80 + "f", b"x")], tarfile.GNU_FORMAT)), LAYER_GZ)]))
    add("valid layer directory entries", oci([(gz(tar_of([("a", None), ("a/b", b"x"), ("c/", None)])), LAYER_GZ)]))
    add("valid layer no end blocks", oci([(gz(tar_of([("a", b"x")], end=False)), LAYER_GZ)]))
    add("valid layer trailing garbage after tar end", oci([(gz(tar_of([("a", b"x")]) + b"garbage"), LAYER_GZ)]))
    add("valid gzip multi-member", oci([(gz(BASE_TAR[:512]) + gz(BASE_TAR[512:]), LAYER_GZ)], diff_ids=[dg(BASE_TAR)]))
    add("valid gzip trailing zeros", oci([(BASE_GZ + b"\0" * 8, LAYER_GZ)], diff_ids=[dg(BASE_TAR)]))
    add("valid layer 3MiB file", oci([(gz(tar_of([("big", os.urandom(1 << 21) + b"\0" * (1 << 20))])), LAYER_GZ)]))
    add("valid tar bytes in raw layer with mtime", oci([(tar_of([("a", b"x")], tarfile.GNU_FORMAT), LAYER)]))
    add("gzip trailing garbage", oci([(BASE_GZ + b"junk", LAYER_GZ)], diff_ids=[dg(BASE_TAR)]))
    add("gzip truncated", oci([(BASE_GZ[:-6], LAYER_GZ)], diff_ids=[dg(BASE_TAR)]))
    add("gzip bad crc", oci([(corrupt(BASE_GZ, len(BASE_GZ) - 8), LAYER_GZ)], diff_ids=[dg(BASE_TAR)]))
    add("gzip bad length", oci([(corrupt(BASE_GZ, len(BASE_GZ) - 1), LAYER_GZ)], diff_ids=[dg(BASE_TAR)]))
    add("gzip media type but raw bytes", oci([(BASE_TAR, LAYER_GZ)], diff_ids=[dg(BASE_TAR)]))
    add("raw media type but gzip bytes", oci([(BASE_GZ, LAYER)], diff_ids=[dg(BASE_TAR)]))
    add("wrong platform", oci([(BASE_GZ, LAYER_GZ)], platform={"architecture": "arm64", "os": "linux"}))
    add("platform extra field", oci([(BASE_GZ, LAYER_GZ)], platform={"architecture": "amd64", "os": "linux", "variant": "v8"}))
    add("diff id not a digest", oci([(BASE_GZ, LAYER_GZ)], diff_ids=["sha256:xyz"]))
    add("diff id wrong but well formed", oci([(BASE_GZ, LAYER_GZ)], diff_ids=["sha256:" + "0" * 64]))
    add("diff id is the compressed digest", oci([(BASE_GZ, LAYER_GZ)], diff_ids=[dg(BASE_GZ)]))
    add("diff id count mismatch", oci([(BASE_GZ, LAYER_GZ)], diff_ids=[dg(BASE_TAR)] * 2))
    add("diff ids not a list", oci([(BASE_GZ, LAYER_GZ)], diff_ids="x"))
    add("unregistered layer media", oci([(BASE_TAR, "application/x-other")]))
    add("layer not a tar", oci([(b"not a tar" * 100, LAYER)]))
    add("layer empty bytes", oci([(b"", LAYER)]))
    add("layer only end blocks", oci([(b"\0" * 1024, LAYER)]))
    add("layer duplicate path", oci([(gz(tar_of([("a", b"x"), ("a", b"y")])), LAYER_GZ)]))
    add("layer duplicate path via dot-slash", oci([(gz(tar_of([("a", b"x"), ("./a", b"y")])), LAYER_GZ)]))
    add("layer duplicate dir and file", oci([(gz(tar_of([("a", None), ("a", b"y")])), LAYER_GZ)]))
    add("layer parent path", oci([(gz(tar_of([("../etc/x", b"x")])), LAYER_GZ)]))
    add("layer inner parent path", oci([(gz(tar_of([("a/../b", b"x")])), LAYER_GZ)]))
    add("layer absolute path", oci([(gz(tar_of([("/etc/x", b"x")])), LAYER_GZ)]))
    add("layer dot root path", oci([(gz(tar_of([("./", None), ("a", b"x")])), LAYER_GZ)]))
    add("layer pax header over 1MiB", oci([(gz(tar_of([("a", b"x", {"comment": "x" * (1 << 21)})])), LAYER_GZ)]))
    add("layer pax headers over 4MiB total", oci([(gz(tar_of([(f"a{i}", b"x", {"comment": "x" * (1 << 20) * 1}) for i in range(5)])), LAYER_GZ)]))
    add("layer pax path override unsafe", oci([(gz(tar_of([("a", b"x", {"path": "../b"})])), LAYER_GZ)]))
    add("layer pax size record", oci([(gz(tar_of([("a", b"x", {"size": "1"})])), LAYER_GZ)]))
    add("layer bad checksum first header", oci([(corrupt(BASE_TAR, 148, ord("9")), LAYER)]))
    add("layer bad checksum later header", oci([(corrupt(tar_of([("a", b"x"), ("b", b"y")]), 1024 + 148, ord("9")), LAYER)]))
    add("layer truncated data", oci([(tar_of([("a", b"x" * 700)])[:900], LAYER)]))
    add("layer truncated header", oci([(tar_of([("a", b"x")])[:1100], LAYER)]))
    add("layer member size not octal", oci([(sized(BASE_TAR, "a", b"zzzzzzzzzzz\0"), LAYER)]))
    add("layer member size base256 big", oci([(sized(BASE_TAR, "a", b"\x80" + b"\xff" * 11), LAYER)]))
    add("layer symlink and hardlink entries", oci([(gz(link_tar()), LAYER_GZ)]))
    add("duplicate outer path", entries=oci([(BASE_GZ, LAYER_GZ)]) + [oci([(BASE_GZ, LAYER_GZ)])[0]])
    add("duplicate json key in layout", oci([(BASE_GZ, LAYER_GZ)], layout=b'{"imageLayoutVersion":"1.0.0","imageLayoutVersion":"1.0.0"}'))
    add("layout version wrong", oci([(BASE_GZ, LAYER_GZ)], layout=jb({"imageLayoutVersion": "2.0.0"})))
    add("layout extra key", oci([(BASE_GZ, LAYER_GZ)], layout=jb({"imageLayoutVersion": "1.0.0", "x": 1})))
    add("layout NaN constant", oci([(BASE_GZ, LAYER_GZ)], layout=b'{"imageLayoutVersion":NaN}'))
    add("layout not json", oci([(BASE_GZ, LAYER_GZ)], layout=b"{"))
    add("layout invalid utf8", oci([(BASE_GZ, LAYER_GZ)], layout=b'{"imageLayoutVersion":"\xff"}'))
    add("layout trailing data", oci([(BASE_GZ, LAYER_GZ)], layout=b'{"imageLayoutVersion":"1.0.0"} x'))
    add("index unknown field", oci([(BASE_GZ, LAYER_GZ)], index_extra={"extra": 1}))
    add("index annotations allowed", oci([(BASE_GZ, LAYER_GZ)], index_extra={"annotations": {"a": "b"}}))
    add("index media type wrong", oci([(BASE_GZ, LAYER_GZ)], index_extra={"mediaType": "x"}))
    add("index media type right", oci([(BASE_GZ, LAYER_GZ)], index_extra={"mediaType": "application/vnd.oci.image.index.v1+json"}))
    add("index schema version 3", oci([(BASE_GZ, LAYER_GZ)], index_extra={"schemaVersion": 3}))
    add("manifest unknown field", oci([(BASE_GZ, LAYER_GZ)], manifest_extra={"extra": 1}))
    add("manifest annotations allowed", oci([(BASE_GZ, LAYER_GZ)], manifest_extra={"annotations": {}}))
    add("manifest media type wrong", oci([(BASE_GZ, LAYER_GZ)], manifest_extra={"mediaType": "x"}))
    add("manifest schema 1", oci([(BASE_GZ, LAYER_GZ)], manifest_extra={"schemaVersion": 1}))
    add("manifest no layers", oci([(BASE_GZ, LAYER_GZ)], manifest_extra={"layers": []}))
    add("config wrong os", oci([(BASE_GZ, LAYER_GZ)], config_extra={"os": "windows"}))
    add("config wrong arch", oci([(BASE_GZ, LAYER_GZ)], config_extra={"architecture": "arm64"}))
    add("config rootfs wrong type", oci([(BASE_GZ, LAYER_GZ)], config_extra={"rootfs": {"type": "x", "diff_ids": [dg(BASE_TAR)]}}))
    es = oci([(BASE_GZ, LAYER_GZ)])
    add("changed blob bytes", [(n, corrupt(d, 20) if n.startswith("blobs/") and d == BASE_GZ else d) for n, d in es])
    add("extra blob", es + [("blobs/sha256/" + "e" * 64, b"x")])
    add("missing blob", [(n, d) for n, d in es if d != BASE_GZ])
    add("missing index", es[:1] + es[2:])
    add("extra file", es + [("extra.txt", b"x")])
    add("unexpected directory", [("other", None)] + es)
    add("blobs directories allowed", [("blobs", None), ("blobs/sha256", None)] + es)
    add("blobs dir trailing slash", [("blobs/", None)] + es)
    add("outer path with dot", [("./" + es[0][0], es[0][1])] + es[1:])
    add("outer path double slash", [("blobs//sha256/" + "e" * 64, b"x")] + es)
    add("outer path parent", [("../x", b"x")] + es)
    add("outer path absolute", [("/x", b"x")] + es)
    add("outer path too long", [("a" * 1100, b"x")] + es)
    add("outer 129 members", es + [(f"blobs/sha256/{i:064x}", b"x") for i in range(125)])
    add("outer 65 blobs", es + [(f"blobs/sha256/{i:064x}", b"x") for i in range(70)])
    add("outer metadata too large", [("oci-layout", b" " * (1 << 21))] + es[1:])
    add("outer symlink member", raw=tar_with_symlink(es))
    add("outer pax over 1MiB", raw=tar_of([("blobs/sha256/" + "d" * 64, b"x", {"comment": "x" * (1 << 21)})] + es))
    add("outer pax small header ok", raw=tar_of([(n, d, {"comment": "x"}) for n, d in es]))
    add("outer gnu format", raw=tar_of(es, tarfile.GNU_FORMAT))
    add("outer ustar format", raw=tar_of(es, tarfile.USTAR_FORMAT))
    add("outer trailing garbage after end", raw=tar_of(es) + b"trailing garbage")
    add("outer no end blocks", raw=tar_of(es, end=False))
    add("outer gzip compressed", raw=gzip.compress(tar_of(es)))
    add("outer bzip2 compressed", raw=bz2.compress(tar_of(es)))
    add("outer xz compressed", raw=lzma.compress(tar_of(es)))
    add("outer empty file", raw=b"")
    add("outer only end blocks", raw=b"\0" * 10240)
    add("outer truncated", raw=tar_of(es)[:3000])
    add("outer not a tar", raw=b"hello world" * 100)
    add("outer bad checksum", raw=corrupt(tar_of(es), 148, ord("9")))
    add("outer is a symlink", setup="archive_symlink")
    add("outer is a directory", setup="archive_dir")
    add("outer missing", setup="archive_missing")
    base_oci = None
    add("same-size descriptor reuse", oci([(BASE_GZ, LAYER_GZ), (BASE_GZ, LAYER)], diff_ids=[dg(BASE_TAR)] * 2))
    rec = lambda f: f
    def edit(path, value, delete=False):
        def apply(record):
            target = record
            for key in path[:-1]:
                target = target[key]
            if delete:
                del target[path[-1]]
            else:
                target[path[-1]] = value
        return apply
    for label, path, value, delete in [
        ("record unknown top field", ["x"], 1, False), ("record missing source", ["source"], None, True),
        ("record schema wrong", ["schema"], "x", False), ("record platform wrong", ["platform"], "linux/arm64", False),
        ("source commit short", ["source", "commit"], "abc", False), ("source commit upper", ["source", "commit"], "A" * 40, False),
        ("source ref not refs", ["source", "ref"], "main", False), ("source commit int", ["source", "commit"], 1, False),
        ("source input missing", ["source", "inputs", "images/job/ubuntu-26.04/README.md"], None, True),
        ("source input extra", ["source", "inputs", "extra"], "b" * 64, False),
        ("source input bad hash", ["source", "inputs", ".velnor/runner.lock"], "zz", False),
        ("source input changed hash", ["source", "inputs", ".velnor/runner.lock"], "b" * 64, False),
        ("workflow repository bad", ["workflow", "repository"], "no-slash", False),
        ("workflow repository other", ["workflow", "repository"], "tailrocks/other", False),
        ("workflow repository id float", ["workflow", "repository_id"], 1.0, False),
        ("workflow repository id zero", ["workflow", "repository_id"], 0, False),
        ("workflow repository id bool", ["workflow", "repository_id"], True, False),
        ("workflow repository id string", ["workflow", "repository_id"], "1", False),
        ("workflow event upper", ["workflow", "event"], "Push", False),
        ("workflow event other", ["workflow", "event"], "schedule", False),
        ("workflow ref wrong repo", ["workflow", "workflow_ref"], "x/y/.github/workflows/a.yml@refs/heads/m", False),
        ("workflow ref no at", ["workflow", "workflow_ref"], "tailrocks/velnor-new/.github/workflows/a.yml", False),
        ("workflow authority bad", ["workflow", "authority_commit"], "c" * 39, False),
        ("workflow authority other", ["workflow", "authority_commit"], "d" * 40, False),
        ("workflow run id zero", ["workflow", "run_id"], 0, False), ("workflow run id other", ["workflow", "run_id"], 11, False),
        ("workflow run attempt other", ["workflow", "run_attempt"], 3, False),
        ("workflow extra", ["workflow", "x"], 1, False),
        ("build builder bad", ["build", "builder"], "docker buildx", False),
        ("build builder with detail", ["build", "builder"], "github.com/docker/buildx v0.38.0 abc123", False),
        ("build builder trailing space", ["build", "builder"], "github.com/docker/buildx v0.38.0 ", False),
        ("build builder newline detail", ["build", "builder"], "github.com/docker/buildx v0.38.0 a\nb", False),
        ("build builder other version", ["build", "builder"], "github.com/docker/buildx v0.39.0", False),
        ("build builder v0.38", ["build", "builder"], "github.com/docker/buildx v0.38", False),
        ("build platform wrong", ["build", "platform"], "linux/arm64", False),
        ("build exporter wrong", ["build", "exporter"], "docker", False),
        ("build provenance on", ["build", "provenance"], "enabled", False),
        ("build compression zstd", ["build", "compression"], "zstd", False),
        ("build force compression false", ["build", "force_compression"], False, False),
        ("build force compression 1", ["build", "force_compression"], 1, False),
        ("build arguments", ["build", "arguments"], ["--x"], False),
        ("archive sha wrong", ["archive", "sha256"], "0" * 64, False),
        ("archive sha upper", ["archive", "sha256"], "A" * 64, False),
        ("archive size wrong", ["archive", "size_bytes"], 1, False),
        ("archive size zero", ["archive", "size_bytes"], 0, False),
        ("archive extra", ["archive", "x"], 1, False),
        ("oci index digest wrong", ["oci", "archive_index_digest"], "0" * 64, False),
        ("oci index digest prefixed", ["oci", "archive_index_digest"], "sha256:" + "0" * 64, False),
        ("oci index size zero", ["oci", "archive_index_size_bytes"], 0, False),
        ("oci manifest size wrong", ["oci", "platform_manifest_size_bytes"], 5, False),
        ("oci config size wrong", ["oci", "image_configuration_size_bytes"], 5, False),
        ("oci manifest digest bad", ["oci", "platform_manifest_digest"], "sha256:xx", False),
        ("oci config digest bad", ["oci", "image_configuration_digest"], "md5:xx", False),
        ("oci layer list empty", ["oci", "layer_digests"], [], False),
        ("oci layer digest bad", ["oci", "layer_digests"], ["x"], False),
        ("oci layer digest other", ["oci", "layer_digests"], ["sha256:" + "0" * 64], False),
        ("oci extra", ["oci", "x"], 1, False),
    ]:
        add(label, base_entries(), record=edit(path, value, delete))
    for name in ["GITHUB_SHA", "GITHUB_REF", "GITHUB_REPOSITORY", "GITHUB_REPOSITORY_ID", "GITHUB_EVENT_NAME",
                 "GITHUB_WORKFLOW_REF", "GITHUB_WORKFLOW_SHA", "GITHUB_RUN_ID", "GITHUB_RUN_ATTEMPT"]:
        add(f"env {name} missing", base_entries(), env={name: None})
        add(f"env {name} empty", base_entries(), env={name: ""})
    for name, value in [("GITHUB_SHA", "b" * 40), ("GITHUB_REF", "refs/heads/other"), ("GITHUB_REF", "heads/x"),
                        ("GITHUB_REPOSITORY", "bad"), ("GITHUB_REPOSITORY_ID", "abc"), ("GITHUB_REPOSITORY_ID", "-1"),
                        ("GITHUB_REPOSITORY_ID", "0"), ("GITHUB_REPOSITORY_ID", "2"), ("GITHUB_EVENT_NAME", "PUSH"),
                        ("GITHUB_EVENT_NAME", "pull_request"), ("GITHUB_WORKFLOW_REF", "tailrocks/velnor-new/x@refs/a"),
                        ("GITHUB_WORKFLOW_SHA", "z" * 40), ("GITHUB_WORKFLOW_SHA", "d" * 40), ("GITHUB_RUN_ID", "0"),
                        ("GITHUB_RUN_ID", "1.5"), ("GITHUB_RUN_ID", "11"), ("GITHUB_RUN_ATTEMPT", "x"), ("GITHUB_RUN_ATTEMPT", "3")]:
        add(f"env {name}={value}", base_entries(), env={name: value})
    for label in ["Dockerfile", "README.md", "build-test.sh", "candidate/candidate.py", "runner.lock"]:
        add(f"root input changed {label}", base_entries(), root=("change", label))
        add(f"root input missing {label}", base_entries(), root=("delete", label))
    add("root lock buildx version other", base_entries(), root=("lock", 'buildx_version = "0.40.0"\n'))
    add("root lock buildx version missing", base_entries(), root=("lock", 'other = 1\n'))
    add("root lock invalid toml", base_entries(), root=("lock", "[["))
    add("record is a symlink", base_entries(), setup="record_symlink")
    add("record is a directory", base_entries(), setup="record_dir")
    add("record missing", base_entries(), setup="record_missing")
    add("record oversized", base_entries(), setup="record_big")
    add("record invalid json", base_entries(), setup="record_garbage")
    add("record duplicate key", base_entries(), setup="record_dup")
    add("record NaN", base_entries(), setup="record_nan")
    add("record not an object", base_entries(), setup="record_array")
    add("record empty", base_entries(), setup="record_empty")
    return out


def link_tar():
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode="w") as archive:
        for name, kind in (("s", tarfile.SYMTYPE), ("h", tarfile.LNKTYPE)):
            info = tarfile.TarInfo(name)
            info.type = kind
            info.linkname = "target"
            archive.addfile(info)
    return buffer.getvalue()


def tar_with_symlink(entries):
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode="w") as archive:
        info = tarfile.TarInfo("link")
        info.type = tarfile.SYMTYPE
        info.linkname = "index.json"
        archive.addfile(info)
        for name, data in entries:
            info = tarfile.TarInfo(name)
            info.size = len(data)
            archive.addfile(info, io.BytesIO(data))
    return buffer.getvalue()


def build_root(root: Path):
    for name in legacy.INPUT_PATHS:
        path = root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(f"content of {name}\n".encode())
    (root / ".velnor/runner.lock").write_text('buildx_version = "0.38.0"\n')


def run(cmd, env):
    done = subprocess.run(cmd, env=env, capture_output=True, text=True, timeout=300)
    return done.returncode, done.stdout, done.stderr


def message(stderr, prefix):
    last = [line for line in stderr.strip().splitlines() if line.strip()]
    if last and last[-1].startswith(prefix):
        return last[-1][len(prefix):]
    return None


def main():
    mismatches, total, message_diffs, stricter = [], 0, [], 0
    base_archive = None
    for case in cases():
        with tempfile.TemporaryDirectory(prefix="parity-") as tmp:
            tmp = Path(tmp)
            root = tmp / "root"
            build_root(root)
            archive, record_path = tmp / "candidate.oci.tar", tmp / "candidate.json"
            raw = case["raw"] if case["raw"] is not None else tar_of(case["entries"] or base_entries())
            archive.write_bytes(raw)
            ref_archive = tmp / "ref.oci.tar"
            ref_archive.write_bytes(tar_of(base_entries()))
            try:
                oci_part = legacy.verify_oci_archive(archive)
                source_archive = archive
            except Exception:
                oci_part = legacy.verify_oci_archive(ref_archive)
                source_archive = ref_archive
            archive_hash, archive_size = legacy._hash_file(archive)
            record = {
                "schema": legacy.SCHEMA,
                "source": {"commit": SHA, "ref": "refs/heads/candidate", "inputs": legacy._source_inputs(root)},
                "workflow": {"repository": "tailrocks/velnor-new", "repository_id": 1, "event": "push",
                            "workflow_ref": ENV["GITHUB_WORKFLOW_REF"], "authority_commit": "c" * 40, "run_id": 10, "run_attempt": 2},
                "platform": "linux/amd64",
                "build": {"builder": "github.com/docker/buildx v0.38.0", "platform": "linux/amd64", "arguments": [],
                          "exporter": "oci", "provenance": "disabled", "compression": "gzip", "force_compression": True},
                "archive": {"sha256": archive_hash, "size_bytes": archive_size},
                "oci": oci_part,
            }
            if case["record"]:
                case["record"](record)
            record_bytes = json.dumps(record).encode()
            setup = case["setup"]
            if setup == "record_garbage": record_bytes = b"{not json"
            elif setup == "record_dup": record_bytes = b'{"schema":"a","schema":"b"}'
            elif setup == "record_nan": record_bytes = b'{"schema":NaN}'
            elif setup == "record_array": record_bytes = b"[]"
            elif setup == "record_empty": record_bytes = b""
            elif setup == "record_big": record_bytes = json.dumps(record).encode() + b" " * (1 << 21)
            if setup == "record_symlink":
                (tmp / "real.json").write_bytes(record_bytes)
                record_path.symlink_to(tmp / "real.json")
            elif setup == "record_dir": record_path.mkdir()
            elif setup == "record_missing": pass
            else: record_path.write_bytes(record_bytes)
            if setup == "archive_symlink":
                archive.rename(tmp / "real.tar"); archive.symlink_to(tmp / "real.tar")
            elif setup == "archive_dir":
                archive.unlink(); archive.mkdir()
            elif setup == "archive_missing":
                archive.unlink()
            if case["root"]:
                kind, arg = case["root"]
                if kind == "lock":
                    (root / ".velnor/runner.lock").write_text(arg)
                else:
                    target = root / (".velnor/runner.lock" if arg == "runner.lock" else "images/job/ubuntu-26.04/" + arg)
                    if kind == "change": target.write_bytes(b"changed\n")
                    else: target.unlink()
            env = dict(os.environ)
            for key in list(env):
                if key.startswith("GITHUB_"): del env[key]
            env.update(ENV)
            for key, value in (case["env"] or {}).items():
                if value is None: env.pop(key, None)
                else: env[key] = value
            args = ["--root", str(root), "--archive", str(archive), "--record", str(record_path)]
            old = run([sys.executable, str(LEGACY), "validate"] + args, env)
            new = run([BINARY, "image", "verify-candidate"] + args, env)
            total += 1
            old_ok, new_ok = old[0] == 0, new[0] == 0
            old_msg, new_msg = message(old[2], "ERROR: "), message(new[2], "velnor-ci: ")
            tag = "ACCEPT" if old_ok else "REJECT"
            if case["name"] in STRICTER and old_ok and not new_ok:
                print(f"stricter      reject {case['name']}: legacy accepts a tar python silently truncates; new refuses")
                stricter += 1
            elif old_ok != new_ok or (old_ok and old[1] != new[1]):
                mismatches.append((case["name"], old_ok, new_ok, old_msg, new_msg))
                print(f"VERDICT-DIFF  {case['name']}: legacy={'accept' if old_ok else 'reject'} new={'accept' if new_ok else 'reject'} | {old_msg or old[2][-120:]!r} | {new_msg!r}")
            elif not old_ok and old_msg != new_msg:
                message_diffs.append((case["name"], old_msg, new_msg))
                print(f"message-diff  {case['name']}: legacy={old_msg!r} new={new_msg!r}")
            else:
                print(f"same          {tag.lower():6} {case['name']}" + (f": {old_msg}" if old_msg else ""))
    print(f"\ncases={total} verdict_mismatches={len(mismatches)} stricter_by_design={stricter} message_diffs={len(message_diffs)}")
    return 1 if mismatches else 0


if __name__ == "__main__":
    sys.exit(main())

#!/usr/bin/env python3
"""Test the job image candidate record and OCI archive."""

import hashlib
import gzip
import bz2
import gzip
import importlib.util
import io
import json
import lzma
from pathlib import Path
import sys
import tarfile
import tempfile
import unittest


MODULE_PATH = Path(__file__).with_name("candidate.py")
SPEC = importlib.util.spec_from_file_location("job_image_candidate", MODULE_PATH)
candidate = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = candidate
SPEC.loader.exec_module(candidate)


def digest(data: bytes) -> str:
    return "sha256:" + hashlib.sha256(data).hexdigest()


def json_bytes(value: object) -> bytes:
    return json.dumps(value, separators=(",", ":")).encode()


def archive_entries(
    platform: dict[str, str] | None = None, diff_ids: object | None = None,
    media_type: str = "application/vnd.oci.image.layer.v1.tar",
    layer_data: bytes | None = None,
    diff_id_data: bytes | None = None,
) -> dict[str, bytes]:
    layer_buffer = io.BytesIO()
    with tarfile.open(fileobj=layer_buffer, mode="w") as layer_tar:
        info = tarfile.TarInfo("job-image")
        info.size = len(b"job-image-ok")
        layer_tar.addfile(info, io.BytesIO(b"job-image-ok"))
    layer = layer_data if layer_data is not None else layer_buffer.getvalue()
    diff_layer = diff_id_data if diff_id_data is not None else layer
    config = json_bytes({
        "architecture": "amd64", "os": "linux",
        "rootfs": {"type": "layers", "diff_ids": diff_ids if diff_ids is not None else [digest(diff_layer)]},
    })
    manifest = json_bytes({
        "schemaVersion": 2,
        "mediaType": "application/vnd.oci.image.manifest.v1+json",
        "config": {"mediaType": "application/vnd.oci.image.config.v1+json", "digest": digest(config), "size": len(config)},
        "layers": [{"mediaType": media_type, "digest": digest(layer), "size": len(layer)}],
    })
    index = json_bytes({
        "schemaVersion": 2,
        "manifests": [{
            "mediaType": "application/vnd.oci.image.manifest.v1+json",
            "digest": digest(manifest), "size": len(manifest),
            "platform": platform or {"architecture": "amd64", "os": "linux"},
        }],
    })
    return {
        "oci-layout": json_bytes({"imageLayoutVersion": "1.0.0"}),
        "index.json": index,
        f"blobs/sha256/{hashlib.sha256(manifest).hexdigest()}": manifest,
        f"blobs/sha256/{hashlib.sha256(config).hexdigest()}": config,
        f"blobs/sha256/{hashlib.sha256(layer).hexdigest()}": layer,
    }


def write_archive(path: Path, entries: dict[str, bytes], duplicate: str | None = None) -> None:
    with tarfile.open(path, "w") as archive:
        for name, data in entries.items():
            info = tarfile.TarInfo(name)
            info.size = len(data)
            archive.addfile(info, io.BytesIO(data))
        if duplicate is not None:
            data = entries[duplicate]
            info = tarfile.TarInfo(duplicate)
            info.size = len(data)
            archive.addfile(info, io.BytesIO(data))


def valid_record(archive_path: Path) -> dict[str, object]:
    archive_hash, archive_size = candidate._hash_file(archive_path)
    oci = candidate.verify_oci_archive(archive_path)
    return {
        "schema": candidate.SCHEMA,
        "source": {
            "commit": "a" * 40, "ref": "refs/heads/candidate",
            "inputs": {name: "b" * 64 for name in candidate.INPUT_PATHS},
        },
        "workflow": {
            "repository": "tailrocks/velnor-new", "repository_id": 1, "event": "push",
            "workflow_ref": "tailrocks/velnor-new/.github/workflows/release.yml@refs/heads/main",
            "authority_commit": "c" * 40, "run_id": 10, "run_attempt": 2,
        },
        "platform": "linux/amd64",
        "build": {
            "builder": "github.com/docker/buildx v0.38.0", "platform": "linux/amd64", "arguments": [],
            "exporter": "oci", "provenance": "disabled",
            "compression": "gzip", "force_compression": True,
        },
        "archive": {"sha256": archive_hash, "size_bytes": archive_size},
        "oci": oci,
    }


class CandidateTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.directory = Path(self.temporary.name)
        self.entries = archive_entries()
        index = json.loads(self.entries["index.json"])
        manifest_digest = index["manifests"][0]["digest"].removeprefix("sha256:")
        manifest = json.loads(self.entries[f"blobs/sha256/{manifest_digest}"])
        self.layer_name = "blobs/sha256/" + manifest["layers"][0]["digest"].removeprefix("sha256:")
        self.archive = self.directory / "candidate.oci.tar"
        write_archive(self.archive, self.entries)

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def test_valid_amd64_archive_and_separate_archive_hash(self) -> None:
        oci = candidate.verify_oci_archive(self.archive)
        record = candidate.validate_record(valid_record(self.archive))
        self.assertEqual(oci["platform_manifest_digest"], record["oci"]["platform_manifest_digest"])
        self.assertNotEqual(record["archive"]["sha256"], oci["platform_manifest_digest"].removeprefix("sha256:"))

    def test_wrong_platform_rejected(self) -> None:
        entries = archive_entries({"architecture": "arm64", "os": "linux"})
        write_archive(self.archive, entries)
        with self.assertRaisesRegex(candidate.CandidateError, "exactly linux/amd64"):
            candidate.verify_oci_archive(self.archive)

    def test_invalid_rootfs_diff_id_rejected(self) -> None:
        entries = archive_entries(diff_ids="x")
        write_archive(self.archive, entries)
        with self.assertRaisesRegex(candidate.CandidateError, "layers do not match"):
            candidate.verify_oci_archive(self.archive)

    def test_wrong_but_well_formed_diff_id_rejected(self) -> None:
        entries = archive_entries(diff_ids=["sha256:" + "0" * 64])
        write_archive(self.archive, entries)
        with self.assertRaisesRegex(candidate.CandidateError, "DiffID does not match"):
            candidate.verify_oci_archive(self.archive)

    def test_unregistered_layer_media_type_rejected(self) -> None:
        entries = archive_entries(media_type="application/vnd.oci.image.layer.v1.evil")
        write_archive(self.archive, entries)
        with self.assertRaisesRegex(candidate.CandidateError, "media type is invalid"):
            candidate.verify_oci_archive(self.archive)

    def test_layer_must_be_a_tar_archive(self) -> None:
        entries = archive_entries(layer_data=b"not a tar archive")
        write_archive(self.archive, entries)
        with self.assertRaisesRegex(candidate.CandidateError, "not a valid tar archive"):
            candidate.verify_oci_archive(self.archive)

    def test_gzip_layer_uses_uncompressed_diff_id(self) -> None:
        plain_entries = archive_entries()
        index = json.loads(plain_entries["index.json"])
        manifest_digest = index["manifests"][0]["digest"].removeprefix("sha256:")
        manifest = json.loads(plain_entries[f"blobs/sha256/{manifest_digest}"])
        plain_layer = plain_entries[
            "blobs/sha256/" + manifest["layers"][0]["digest"].removeprefix("sha256:")
        ]
        entries = archive_entries(
            media_type="application/vnd.oci.image.layer.v1.tar+gzip",
            layer_data=gzip.compress(plain_layer), diff_id_data=plain_layer,
        )
        write_archive(self.archive, entries)
        self.assertEqual(len(candidate.verify_oci_archive(self.archive)["layer_digests"]), 1)

    def test_layer_rejects_duplicate_paths(self) -> None:
        layer_buffer = io.BytesIO()
        with tarfile.open(fileobj=layer_buffer, mode="w") as layer_tar:
            for content in (b"first", b"second"):
                info = tarfile.TarInfo("same-path")
                info.size = len(content)
                layer_tar.addfile(info, io.BytesIO(content))
        entries = archive_entries(layer_data=layer_buffer.getvalue())
        write_archive(self.archive, entries)
        with self.assertRaisesRegex(candidate.CandidateError, "repeats a path"):
            candidate.verify_oci_archive(self.archive)

    def test_layer_pax_header_is_bounded_before_tar_parse(self) -> None:
        layer_buffer = io.BytesIO()
        with tarfile.open(fileobj=layer_buffer, mode="w", format=tarfile.PAX_FORMAT) as layer_tar:
            info = tarfile.TarInfo("small-file")
            info.size = 1
            info.pax_headers = {"comment": "x" * 65}
            layer_tar.addfile(info, io.BytesIO(b"x"))
        entries = archive_entries(layer_data=layer_buffer.getvalue())
        write_archive(self.archive, entries)
        original_limit = candidate.MAX_TAR_EXTENSION_BYTES
        candidate.MAX_TAR_EXTENSION_BYTES = 64
        try:
            with self.assertRaisesRegex(candidate.CandidateError, "extension header exceeds"):
                candidate.verify_oci_archive(self.archive)
        finally:
            candidate.MAX_TAR_EXTENSION_BYTES = original_limit

    def test_outer_archive_pax_header_is_bounded_before_tar_parse(self) -> None:
        original_limit = candidate.MAX_TAR_EXTENSION_BYTES
        candidate.MAX_TAR_EXTENSION_BYTES = 64
        try:
            with tarfile.open(self.archive, "w", format=tarfile.PAX_FORMAT) as archive:
                for name, data in self.entries.items():
                    info = tarfile.TarInfo(name)
                    info.size = len(data)
                    if name == "oci-layout":
                        info.pax_headers = {"comment": "x" * 65}
                    archive.addfile(info, io.BytesIO(data))
            with self.assertRaisesRegex(candidate.CandidateError, "extension header exceeds"):
                candidate.verify_oci_archive(self.archive)
        finally:
            candidate.MAX_TAR_EXTENSION_BYTES = original_limit

    def test_outer_archive_compression_is_rejected_before_tar_parse(self) -> None:
        raw_archive = self.archive.read_bytes()
        for name, compress in (
            ("gzip", gzip.compress),
            ("bzip2", bz2.compress),
            ("xz", lzma.compress),
        ):
            with self.subTest(format=name):
                self.archive.write_bytes(compress(raw_archive))
                with self.assertRaisesRegex(candidate.CandidateError, "uncompressed tar"):
                    candidate.verify_oci_archive(self.archive)

    def test_duplicate_archive_path_rejected(self) -> None:
        write_archive(self.archive, self.entries, "index.json")
        with self.assertRaisesRegex(candidate.CandidateError, "Duplicate archive path"):
            candidate.verify_oci_archive(self.archive)

    def test_duplicate_json_key_rejected(self) -> None:
        entries = dict(self.entries)
        entries["index.json"] = b'{"schemaVersion":2,"schemaVersion":2,"manifests":[]}'
        write_archive(self.archive, entries)
        with self.assertRaisesRegex(candidate.CandidateError, "Duplicate JSON key"):
            candidate.verify_oci_archive(self.archive)

    def test_changed_blob_rejected(self) -> None:
        entries = dict(self.entries)
        entries[self.layer_name] = b"bad-layer-bytes"
        write_archive(self.archive, entries)
        with self.assertRaises(candidate.CandidateError):
            candidate.verify_oci_archive(self.archive)

    def test_extra_blob_rejected(self) -> None:
        entries = dict(self.entries)
        entries[f"blobs/sha256/{hashlib.sha256(b'extra').hexdigest()}"] = b"extra"
        write_archive(self.archive, entries)
        with self.assertRaisesRegex(candidate.CandidateError, "missing or unreferenced"):
            candidate.verify_oci_archive(self.archive)

    def test_missing_blob_rejected(self) -> None:
        entries = {name: data for name, data in self.entries.items() if name != self.layer_name}
        write_archive(self.archive, entries)
        with self.assertRaisesRegex(candidate.CandidateError, "missing or unreferenced"):
            candidate.verify_oci_archive(self.archive)

    def test_record_rejects_unknown_field(self) -> None:
        record = valid_record(self.archive)
        record["unexpected"] = True
        with self.assertRaisesRegex(candidate.CandidateError, "unknown or missing"):
            candidate.validate_record(record)

    def test_output_path_rejects_source_tree_and_file_symlink(self) -> None:
        source = (self.directory / "source").resolve()
        source.mkdir()
        with self.assertRaisesRegex(candidate.CandidateError, "outside the source tree"):
            candidate._output_path(source, str(source / "archive.tar"))
        target = self.directory / "target.tar"
        target.write_bytes(b"owned")
        link = self.directory / "candidate.tar"
        link.symlink_to(target)
        with self.assertRaisesRegex(candidate.CandidateError, "cannot use symlinks"):
            candidate._output_path(source, str(link))
        dangling = self.directory / "dangling.tar"
        dangling.symlink_to(self.directory / "missing.tar")
        with self.assertRaisesRegex(candidate.CandidateError, "cannot use symlinks"):
            candidate._output_path(source, str(dangling))

    def test_record_rejects_unrecorded_build_arguments(self) -> None:
        record = valid_record(self.archive)
        record["build"]["arguments"] = ["TOKEN=secret"]
        with self.assertRaisesRegex(candidate.CandidateError, "empty build argument list"):
            candidate.validate_record(record)

    def test_buildx_banner_does_not_replace_binary_hash(self) -> None:
        binary = self.directory / "fake-buildx"
        binary.write_bytes(b"fake executable")
        docker = self.directory / "docker"
        docker.write_text(
            "#!/usr/bin/env python3\n"
            "import json, sys\n"
            "if sys.argv[1:] == ['buildx', 'version']:\n"
            " print('github.com/docker/buildx v0.38.0')\n"
            "else:\n"
            f" print(json.dumps([{{'Name': 'buildx', 'Version': 'v0.38.0', 'Path': {str(binary)!r}}}]))\n"
        )
        docker.chmod(0o755)
        with self.assertRaisesRegex(candidate.CandidateError, "binary does not match"):
            candidate._buildx_details(str(docker), candidate.ROOT)

    def test_archive_size_limit_rejects_oversized_input(self) -> None:
        original_limit = candidate.MAX_ARCHIVE_BYTES
        candidate.MAX_ARCHIVE_BYTES = 1
        try:
            with self.assertRaisesRegex(candidate.CandidateError, "file size limit"):
                candidate.verify_oci_archive(self.archive)
        finally:
            candidate.MAX_ARCHIVE_BYTES = original_limit

    def test_each_build_input_change_is_detected(self) -> None:
        root = self.directory / "source"
        for relative in candidate.INPUT_PATHS:
            path = root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(relative.encode())
        record = valid_record(self.archive)
        record["source"]["inputs"] = candidate._source_inputs(root)
        for relative in candidate.INPUT_PATHS:
            with self.subTest(path=relative):
                path = root / relative
                path.write_bytes(path.read_bytes() + b"changed")
                with self.assertRaisesRegex(candidate.CandidateError, "Build input bytes changed"):
                    candidate._verify_source_inputs(record, root)
                path.write_bytes(relative.encode())


if __name__ == "__main__":
    unittest.main(verbosity=2)

"""Focused saved-archive identity tests for the image release producer."""

import hashlib
import io
import json
import pathlib
import tarfile
import tempfile
import unittest

import write_manifest as producer


SOURCE_SHA = "a" * 40


def json_bytes(value):
    return json.dumps(value, separators=(",", ":")).encode()


def add_blob(files, data):
    digest = hashlib.sha256(data).hexdigest()
    files[f"blobs/sha256/{digest}"] = data
    return f"sha256:{digest}"


def archive_fixture(platform=None, extra=None, duplicate=False, referrer=False):
    config = {
        "architecture": "amd64",
        "config": {
            "User": producer.IMAGE_USER,
            "Env": producer.DEFAULT_ENV,
            "Entrypoint": producer.ENTRYPOINT,
            "Labels": {"org.opencontainers.image.revision": SOURCE_SHA},
        },
        "os": "linux",
        "rootfs": {"type": "layers", "diff_ids": []},
    }
    config_bytes = json_bytes(config)
    config_digest = add_blob({}, config_bytes)
    layer_bytes = b"synthetic layer payload"
    layer_digest = add_blob({}, layer_bytes)
    manifest = {
        "schemaVersion": 2,
        "mediaType": "application/vnd.docker.distribution.manifest.v2+json",
        "config": {
            "mediaType": "application/vnd.docker.container.image.v1+json",
            "digest": config_digest,
            "size": len(config_bytes),
        },
        "layers": [
            {
                "mediaType": "application/vnd.docker.image.rootfs.diff.tar.gzip",
                "digest": layer_digest,
                "size": len(layer_bytes),
            }
        ],
    }
    manifest_bytes = json_bytes(manifest)
    manifest_digest = add_blob({}, manifest_bytes)
    files = {
        "oci-layout": json_bytes({"imageLayoutVersion": "1.0.0"}),
        "manifest.json": json_bytes(
            [
                {
                    "Config": f"blobs/sha256/{config_digest.removeprefix('sha256:')}",
                    "RepoTags": [producer.IMAGE_TAG],
                    "Layers": [f"blobs/sha256/{layer_digest.removeprefix('sha256:')}"],
                }
            ]
        ),
    }
    files.update({
        f"blobs/sha256/{config_digest.removeprefix('sha256:')}": config_bytes,
        f"blobs/sha256/{layer_digest.removeprefix('sha256:')}": layer_bytes,
        f"blobs/sha256/{manifest_digest.removeprefix('sha256:')}": manifest_bytes,
    })
    index_descriptor = {
        "mediaType": manifest["mediaType"],
        "digest": manifest_digest,
        "size": len(manifest_bytes),
    }
    if platform is not None:
        index_descriptor["platform"] = platform
    index_descriptors = [index_descriptor]
    if referrer:
        statement = json_bytes(
            {
                "_type": "https://in-toto.io/Statement/v1",
                "subject": [
                    {
                        "name": producer.IMAGE_TAG,
                        "digest": {"sha256": manifest_digest[7:]},
                    }
                ],
                "predicateType": "https://slsa.dev/provenance/v1",
                "predicate": {},
            }
        )
        referrer_config = b"{}"
        referrer_config_digest = add_blob(files, referrer_config)
        statement_digest = add_blob(files, statement)
        referrer_manifest = {
            "schemaVersion": 2,
            "mediaType": "application/vnd.oci.image.manifest.v1+json",
            "config": {
                "mediaType": "application/vnd.oci.empty.v1+json",
                "digest": referrer_config_digest,
                "size": len(referrer_config),
            },
            "layers": [
                {
                    "mediaType": "application/vnd.in-toto+json",
                    "digest": statement_digest,
                    "size": len(statement),
                }
            ],
        }
        referrer_bytes = json_bytes(referrer_manifest)
        referrer_digest = add_blob(files, referrer_bytes)
        index_descriptors.append(
            {
                "mediaType": "application/vnd.oci.image.manifest.v1+json",
                "digest": referrer_digest,
                "size": len(referrer_bytes),
                "annotations": {"io.containerd.manifest.subject": manifest_digest},
            }
        )
    files["index.json"] = json_bytes(
        {
            "schemaVersion": 2,
            "mediaType": "application/vnd.oci.image.index.v1+json",
            "manifests": index_descriptors,
        }
    )
    if extra is not None:
        files[extra] = b"unexpected"
    return files


def write_tar(path, files, duplicate=False):
    with tarfile.open(path, "w:") as archive:
        for name in ("blobs", "blobs/sha256"):
            info = tarfile.TarInfo(name)
            info.type = tarfile.DIRTYPE
            archive.addfile(info)
        for name, data in files.items():
            info = tarfile.TarInfo(name)
            info.size = len(data)
            archive.addfile(info, io.BytesIO(data))
            if duplicate and name == "manifest.json":
                archive.addfile(info, io.BytesIO(data))


def inspect_shape(identity, mode):
    config = identity["config_document"]["config"]
    image = {
        "Os": "linux",
        "Architecture": "amd64",
        "Config": config,
        "Id": identity["config_digest"],
    }
    if mode == "containerd":
        image["Id"] = identity["image_manifest_digest"]
        image["Descriptor"] = {
            "mediaType": identity["image_manifest_media_type"],
            "digest": identity["image_manifest_digest"],
        }
    return image


class ArchiveIdentityTests(unittest.TestCase):
    def with_archive(self, files, check, duplicate=False):
        with tempfile.TemporaryDirectory() as directory:
            path = pathlib.Path(directory) / "image.tar"
            write_tar(path, files, duplicate)
            check(path)

    def test_accepts_classic_and_containerd_inspect_identities(self):
        def check(path):
            identity = producer.archive_identity(path)
            self.assertNotEqual(identity["image_manifest_digest"], identity["config_digest"])
            producer.require_image(inspect_shape(identity, "classic"), SOURCE_SHA, identity)
            producer.require_image(inspect_shape(identity, "containerd"), SOURCE_SHA, identity)

        self.with_archive(archive_fixture({"os": "linux", "architecture": "amd64"}), check)

    def test_accepts_only_referrers_bound_to_the_selected_image(self):
        files = archive_fixture(referrer=True)
        self.with_archive(files, lambda path: producer.archive_identity(path))

        index = json.loads(files["index.json"])
        index["manifests"][1]["annotations"]["io.containerd.manifest.subject"] = (
            "sha256:" + "0" * 64
        )
        files["index.json"] = json_bytes(index)
        self.with_archive(
            files,
            lambda path: self.assertRaises(
                ValueError, producer.archive_identity, path
            ),
        )

    def test_rejects_wrong_loaded_id_and_descriptor(self):
        def check(path):
            identity = producer.archive_identity(path)
            image = inspect_shape(identity, "classic")
            image["Id"] = "sha256:" + "0" * 64
            with self.assertRaises(ValueError):
                producer.require_image(image, SOURCE_SHA, identity)
            image = inspect_shape(identity, "containerd")
            image["Descriptor"]["digest"] = identity["config_digest"]
            with self.assertRaises(ValueError):
                producer.require_image(image, SOURCE_SHA, identity)
            image = inspect_shape(identity, "classic")
            image["Descriptor"] = None
            with self.assertRaises(ValueError):
                producer.require_image(image, SOURCE_SHA, identity)

        self.with_archive(archive_fixture(), check)

    def test_rejects_wrong_platform_and_extra_or_duplicate_paths(self):
        for files, duplicate in (
            (archive_fixture({"os": "linux", "architecture": "arm64"}), False),
            (archive_fixture(extra="unexpected.txt"), False),
            (archive_fixture(), True),
        ):
            with self.subTest(duplicate=duplicate, names=tuple(files)):
                self.with_archive(
                    files,
                    lambda path: self.assertRaises(ValueError, producer.archive_identity, path),
                    duplicate,
                )

    def test_rejects_ambiguous_image_descriptors_and_duplicate_json_keys(self):
        files = archive_fixture({"os": "linux", "architecture": "amd64"})
        index = json.loads(files["index.json"])
        index["manifests"].append(index["manifests"][0])
        files["index.json"] = json_bytes(index)
        self.with_archive(
            files,
            lambda path: self.assertRaises(
                ValueError, producer.archive_identity, path
            ),
        )

        files = archive_fixture()
        files["oci-layout"] = (
            b'{"imageLayoutVersion":"1.0.0","imageLayoutVersion":"1.0.0"}'
        )
        self.with_archive(
            files,
            lambda path: self.assertRaises(
                ValueError, producer.archive_identity, path
            ),
        )


if __name__ == "__main__":
    unittest.main()

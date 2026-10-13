"""Run with python3 -I oci_archive_test.py; no Docker or registry required."""
import hashlib
import io
import json
import pathlib
import tarfile
import tempfile
import types
import sys
import unittest

DIRECTORY = pathlib.Path(__file__).parent
for module_name in ("oci_digest", "oci_archive"):
    module = types.ModuleType(module_name)
    sys.modules[module_name] = module
    source_path = DIRECTORY / (module_name + ".py")
    exec(compile(source_path.read_text(), str(source_path), "exec"), module.__dict__)
GATES = sys.modules["oci_archive"].__dict__
MEDIA = GATES["MANIFEST_MEDIA"]
INDEX = GATES["INDEX_MEDIA"]
SOURCE = "a" * 40
URL = "https://github.com/example/project"


def encoded(document):
    return json.dumps(document, separators=(",", ":"), sort_keys=True).encode()


class Fixture:
    def __init__(self, mutation=None):
        self.files = {}
        self.mutation = mutation
        config = {"os": "linux", "architecture": "amd64", "config": {"Labels": {
            "org.opencontainers.image.version": "1.2.3",
            "org.opencontainers.image.revision": SOURCE,
            "org.opencontainers.image.source": URL}}}
        if mutation == "labels":
            config["config"]["Labels"]["org.opencontainers.image.revision"] = "b" * 40
        image = self.manifest(config, [self.blob(b"layer", "application/vnd.oci.image.layer.v1.tar")])
        image["platform"] = {"os": "linux", "architecture": "amd64"}
        child = image["digest"]
        provenance = {"buildType": "https://mobyproject.org/buildkit@v1", "builder": {},
                      "invocation": {"environment": {"platform": "linux/amd64"}},
                      "metadata": {"https://mobyproject.org/buildkit@v1#metadata": {"vcs": {"revision": SOURCE, "source": URL}}}}
        if mutation == "git_source":
            provenance["metadata"]["https://mobyproject.org/buildkit@v1#metadata"]["vcs"]["source"] = URL + ".git"
        if mutation == "wrong_source":
            provenance["metadata"]["https://mobyproject.org/buildkit@v1#metadata"]["vcs"]["source"] = URL + "/other"
        if mutation == "provenance_source":
            provenance["metadata"] = {}
        sbom = {"SPDXID": "SPDXRef-DOCUMENT", "spdxVersion": "SPDX-2.3", "creationInfo": {}}
        statements = []
        for kind, predicate in [("https://slsa.dev/provenance/v0.2", provenance), (GATES["SPDX_TYPE"], sbom)]:
            statement = {"_type": "https://in-toto.io/Statement/v0.1", "predicateType": kind,
                         "subject": [{"name": "pkg:docker/example/image", "digest": {"sha256": child[7:]}}], "predicate": predicate}
            if mutation == "subject":
                statement["subject"][0]["digest"]["sha256"] = "b" * 64
            descriptor = self.blob(encoded(statement), GATES["STATEMENT_MEDIA"])
            descriptor["annotations"] = {"in-toto.io/predicate-type": kind}
            statements.append(descriptor)
        if mutation == "coverage":
            statements.pop()
        empty = self.blob(b"{} " if mutation == "empty_config_space" else b"{}", GATES["EMPTY_MEDIA"])
        if mutation != "empty_config_space":
            empty["data"] = "e30="
        if mutation == "inline":
            del self.files["blobs/sha256/" + empty["digest"][7:]]
        attached = {key: image[key] for key in ("mediaType", "digest", "size")}
        if mutation == "artifact_subject":
            attached["size"] += 1
        attestation = self.blob(encoded({"schemaVersion": 2, "mediaType": MEDIA,
                                       "artifactType": GATES["ATTESTATION_ARTIFACT"], "subject": attached,
                                       "config": empty, "layers": statements}), MEDIA)
        attestation["platform"] = {"os": "unknown", "architecture": "unknown"}
        attestation["annotations"] = {"vnd.docker.reference.type": "attestation-manifest", "vnd.docker.reference.digest": child}
        if mutation == "descriptor_subject":
            attestation["annotations"]["vnd.docker.reference.digest"] = "sha256:" + "b" * 64
        root = self.blob(encoded({"schemaVersion": 2, "mediaType": INDEX, "manifests": [image, attestation]}), INDEX)
        self.digest = root["digest"]
        if mutation == "nested":
            root = self.blob(encoded({"schemaVersion": 2, "mediaType": INDEX, "manifests": [root]}), INDEX)
            self.digest = root["digest"]
        if mutation == "size":
            root["size"] += 1
        if mutation == "upper_digest":
            root["digest"] = root["digest"].upper()
        self.files["index.json"] = encoded({"schemaVersion": 2, "manifests": [root]})
        self.files["oci-layout"] = encoded({"imageLayoutVersion": "1.0.0"})
        if mutation == "layout":
            self.files["oci-layout"] = encoded({"imageLayoutVersion": "9.0.0"})
        if mutation == "json_constant":
            self.files["oci-layout"] = b'{"imageLayoutVersion":"1.0.0","x":NaN}'
        if mutation == "json_duplicate":
            self.files["oci-layout"] = b'{"imageLayoutVersion":"1.0.0","imageLayoutVersion":"1.0.0"}'
        if mutation == "extra":
            self.blob(b"unreachable", GATES["CONFIG_MEDIA"])
        if mutation == "missing":
            del self.files["blobs/sha256/" + child[7:]]

    def blob(self, data, media):
        digest = "sha256:" + hashlib.sha256(data).hexdigest()
        self.files["blobs/sha256/" + digest[7:]] = data
        return {"mediaType": media, "digest": digest, "size": len(data)}

    def manifest(self, config, layers):
        descriptor = self.blob(encoded(config), GATES["CONFIG_MEDIA"])
        return self.blob(encoded({"schemaVersion": 2, "mediaType": MEDIA, "config": descriptor, "layers": layers}), MEDIA)

    def tar(self, path, extra=None, pax=None):
        with tarfile.open(path, "w", format=tarfile.PAX_FORMAT if pax else tarfile.USTAR_FORMAT) as archive:
            for name, payload in self.files.items():
                info = tarfile.TarInfo(name)
                info.size = len(payload)
                if pax:
                    info.pax_headers = pax
                archive.addfile(info, io.BytesIO(payload))
            if extra:
                info, data = extra
                archive.addfile(info, io.BytesIO(data))


class ArchiveTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.path = pathlib.Path(self.temp.name) / "oci.tar"

    def tearDown(self):
        self.temp.cleanup()

    def validate(self, fixture, digest=None):
        return GATES["validate_archive"](self.path, digest or fixture.digest, "image", "ghcr.io/example/image", "1.2.3", SOURCE, "amd64", URL)

    def rejects(self, fixture, expected, extra=None):
        fixture.tar(self.path, extra)
        with self.assertRaisesRegex(GATES["GateError"], expected):
            self.validate(fixture)

    def test_valid_archive_streams_and_immutable_metadata(self):
        fixture = Fixture()
        fixture.tar(self.path)
        with self.validate(fixture) as archive:
            self.assertEqual(archive.digest, fixture.digest)
            self.assertEqual(len(archive.manifests), 3)
            for blob in archive.blobs.values():
                with blob.open() as stream:
                    self.assertEqual("sha256:" + hashlib.sha256(stream.read()).hexdigest(), blob.digest)
            with self.assertRaises(TypeError):
                archive.blobs["other"] = None

    def test_rejected_content_and_closure(self):
        for mutation, code in [("labels", "config_labels"), ("subject", "statement_subject"), ("provenance_source", "provenance_source"),
                               ("descriptor_subject", "attestation_subject"), ("artifact_subject", "subject_descriptor"), ("coverage", "coverage"),
                               ("layout", "layout_version"), ("extra", "blob_closure"), ("missing", "descriptor_binding"),
                               ("size", "descriptor_binding"), ("upper_digest", "archive_descriptor"),
                               ("json_constant", "archive_json"), ("json_duplicate", "duplicate_json_key"),
                               ("empty_config_space", "descriptor_media"), ("wrong_source", "provenance_source")]:
            with self.subTest(mutation=mutation):
                self.rejects(Fixture(mutation), code)

    def test_pinned_buildx_git_source_spelling(self):
        fixture = Fixture("git_source")
        fixture.tar(self.path)
        with self.validate(fixture) as archive:
            self.assertEqual(archive.digest, fixture.digest)

    def test_bounded_pax_metadata_and_rejected_path_override(self):
        fixture = Fixture()
        fixture.tar(self.path, pax={"mtime": "0.125"})
        with self.validate(fixture) as archive:
            self.assertEqual(archive.digest, fixture.digest)
        fixture.tar(self.path, pax={"path": "../escape"})
        with self.assertRaisesRegex(GATES["GateError"], "archive_pax_key"):
            self.validate(fixture)
        info = tarfile.TarInfo("PaxHeaders.0/unused")
        info.type, info.size = tarfile.XHDTYPE, 11
        self.rejects(fixture, "archive_pax_dangling", (info, b"11 mtime=0\n"))

    def test_inline_empty_config_preserved(self):
        fixture = Fixture("inline")
        fixture.tar(self.path)
        with self.validate(fixture) as archive:
            inline = [blob for blob in archive.blobs.values() if blob.data is not None]
            self.assertEqual(len(inline), 1)
            self.assertEqual(inline[0].size, 2)
            with inline[0].open() as stream:
                self.assertEqual(stream.read(), b"{}")

    def test_recursive_indexes_preserve_postorder(self):
        fixture = Fixture("nested")
        fixture.tar(self.path)
        with self.validate(fixture) as archive:
            self.assertEqual(len(archive.manifests), 4)
            self.assertEqual(archive.manifests[0].digest, fixture.digest)
            self.assertEqual(json.loads(archive.manifests[-1].metadata())["mediaType"], INDEX)

    def test_configured_bounds(self):
        fixture = Fixture()
        fixture.tar(self.path)
        for key in ("MAX_OCI_ENTRIES", "MAX_OCI_METADATA", "MAX_OCI_BLOBS"):
            old = GATES[key]
            try:
                GATES[key] = 1
                with self.assertRaisesRegex(GATES["GateError"], "bound"):
                    self.validate(fixture)
            finally:
                GATES[key] = old

    def test_paths_links_devices_duplicates(self):
        for name, kind, link in [("../escape", tarfile.REGTYPE, ""), ("/absolute", tarfile.REGTYPE, ""),
                                 ("index.json", tarfile.REGTYPE, ""), ("alias", tarfile.SYMTYPE, "index.json"),
                                 ("alias", tarfile.LNKTYPE, "index.json"), ("device", tarfile.CHRTYPE, "")]:
            with self.subTest(name=name, kind=kind):
                info = tarfile.TarInfo(name)
                info.type, info.linkname = kind, link
                self.rejects(Fixture(), "archive_", (info, b""))

    def test_digest_and_header(self):
        fixture = Fixture()
        fixture.tar(self.path)
        with self.assertRaisesRegex(GATES["GateError"], "root_digest"):
            self.validate(fixture, "sha256:" + "b" * 64)
        payload = bytearray(self.path.read_bytes())
        payload[0] ^= 1
        self.path.write_bytes(payload)
        with self.assertRaisesRegex(GATES["GateError"], "header_checksum"):
            self.validate(fixture)

    def test_blob_corruption_and_trailing_bytes(self):
        fixture = Fixture()
        fixture.tar(self.path)
        payload = bytearray(self.path.read_bytes())
        payload[512] ^= 1
        self.path.write_bytes(payload)
        with self.assertRaisesRegex(GATES["GateError"], "blob_digest"):
            self.validate(fixture)
        fixture.tar(self.path)
        with self.path.open("ab") as handle:
            handle.write(b"evil")
        with self.assertRaisesRegex(GATES["GateError"], "trailing_data"):
            self.validate(fixture)

    def test_mutated_file_and_symlink(self):
        fixture = Fixture()
        fixture.tar(self.path)
        with self.validate(fixture) as archive:
            with self.path.open("ab") as handle:
                handle.write(b"evil")
            with self.assertRaisesRegex(GATES["GateError"], "archive_changed"):
                archive.manifests[0].metadata()
        original = self.path.with_name("original.tar")
        self.path.rename(original)
        self.path.symlink_to(original)
        with self.assertRaises(OSError):
            self.validate(fixture)


if __name__ == "__main__":
    unittest.main()

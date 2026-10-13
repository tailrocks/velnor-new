"""Actual namespace tests for the postbuild full-CI publication boundary."""
import importlib.util
import os
from pathlib import Path
import sys
import unittest
from unittest.mock import patch


def load(name):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(name + ".py"))
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


digest = load("oci_digest")
load("oci_archive")
load("oci_registry")
publisher = load("oci_platform_publish")
CONFIG = {"repository": "example/repo", "ci_workflow": "ci.yml", "default_branch": "main"}
ENV = {
    "REPOSITORY": "example/repo", "IMAGE": "example/base", "IMAGE_ID": "base",
    "ARCH": "amd64", "VERSION": "1.2.3", "SOURCE_SHA": "a" * 40,
    "REF": "refs/tags/v1.2.3", "DIGEST": "sha256:" + "b" * 64,
    "DOCKER_CONFIG": "/tmp/runner/velnor/oci-docker",
    "OCI_ARCHIVE": "/tmp/runner/velnor/oci-base-amd64.tar",
}


class Archive:
    def __init__(self, trace):
        self.trace = trace

    def __enter__(self):
        self.trace.append("validated_archive")
        return self

    def __exit__(self, *args):
        self.trace.append("closed_archive")


class PublicationBoundary(unittest.TestCase):
    def setUp(self):
        self.env = patch.dict(os.environ, ENV, clear=True)
        self.env.start()
        self.addCleanup(self.env.stop)

    def test_full_admission_before_first_mutation_and_each_manifest(self):
        trace = []
        def publish(archive, image, docker_config, first, manifest):
            first()
            trace.append("blob_write")
            manifest()
            trace.append("child_write")
            manifest()
            trace.append("root_write")
            return ENV["DIGEST"]
        with patch.object(publisher, "validate_archive", return_value=Archive(trace)) as validate, \
             patch.object(publisher, "publish_admission_main", side_effect=lambda cfg: trace.append("full_ci")) as gate, \
             patch.object(publisher, "publish_verified", side_effect=publish):
            publisher.main(CONFIG)
        self.assertEqual(trace, ["validated_archive", "full_ci", "blob_write", "full_ci", "child_write", "full_ci", "root_write", "closed_archive"])
        self.assertEqual(gate.call_count, 3)
        self.assertEqual(validate.call_args.args[-1], "https://github.com/example/repo")
        self.assertTrue(all(call.args == (CONFIG,) for call in gate.call_args_list))

    def test_failed_ci_prevents_first_write_and_closes_archive(self):
        trace = []
        def publish(archive, image, docker_config, first, manifest):
            first()
            trace.append("write")
        with patch.object(publisher, "validate_archive", return_value=Archive(trace)), \
             patch.object(publisher, "publish_admission_main", side_effect=digest.GateError("ci_failed")), \
             patch.object(publisher, "publish_verified", side_effect=publish):
            with self.assertRaisesRegex(digest.GateError, "ci_failed"):
                publisher.main(CONFIG)
        self.assertEqual(trace, ["validated_archive", "closed_archive"])

    def test_identity_path_and_ci_binding_reject_before_archive_or_network(self):
        mutations = [
            {"REF": "refs/tags/v9.0.0"}, {"OCI_ARCHIVE": "/tmp/other.tar"},
            {"DOCKER_CONFIG": "/tmp/../runner/velnor/oci-docker"}, {"SOURCE_SHA": "BAD"},
            {"DIGEST": "sha256:" + "B" * 64}, {"ARCH": "unknown"},
        ]
        for mutation in mutations:
            with self.subTest(mutation=mutation), patch.dict(os.environ, mutation), \
                 patch.object(publisher, "validate_archive", side_effect=AssertionError("opened archive")), \
                 patch.object(publisher, "publish_verified", side_effect=AssertionError("network")):
                with self.assertRaises(digest.GateError):
                    publisher.main(CONFIG)
        with patch.object(publisher, "validate_archive", side_effect=AssertionError("opened archive")):
            with self.assertRaisesRegex(digest.GateError, "publisher_ci"):
                publisher.main(dict(CONFIG, ci_workflow="-"))

    def test_publication_cannot_change_built_digest(self):
        trace = []
        with patch.object(publisher, "validate_archive", return_value=Archive(trace)), \
             patch.object(publisher, "publish_verified", return_value="sha256:" + "c" * 64):
            with self.assertRaisesRegex(digest.GateError, "publisher_result_digest"):
                publisher.main(CONFIG)
        self.assertEqual(trace[-1], "closed_archive")


if __name__ == "__main__":
    unittest.main()

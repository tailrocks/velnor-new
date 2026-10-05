#!/usr/bin/python3
"""Read-only cross-schema checks for the reviewed preseed asset inventory."""

import copy
import importlib.util
import json
import pathlib
import sys
import tarfile
from unittest.mock import patch

sys.dont_write_bytecode = True
task = pathlib.Path("/root/.velnor-generator-preview-20261005")
module_path = task / "preseed-preview-materialize.py"
spec = importlib.util.spec_from_file_location("materializer", module_path)
if spec is None or spec.loader is None:
    raise SystemExit("asset contract: materializer import failed")
materializer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(materializer)

plan = json.loads((task / "rootfs-stage-plan.json").read_bytes())
provenance = json.loads((task / "official-tool-asset-provenance.json").read_bytes())
materializer.verify_asset_provenance(plan, provenance)
path_counts = materializer.preflight_staging_inputs(plan)


def rejects(mutator):
    changed_plan = copy.deepcopy(plan)
    changed_provenance = copy.deepcopy(provenance)
    mutator(changed_plan, changed_provenance)
    try:
        materializer.verify_asset_provenance(changed_plan, changed_provenance)
    except SystemExit:
        return
    raise AssertionError("mutated asset contract was accepted")


rejects(lambda changed_plan, changed: changed_plan.update(tool_provenance_sha256="0" * 64))
rejects(lambda changed_plan, changed: changed["mise"].update(local_sha256="0" * 64))
rejects(lambda changed_plan, changed: changed["rust"]["components"].pop())
rejects(lambda changed_plan, changed: changed["rust"]["components"][0].update(archive="unknown.tar.xz"))
rejects(lambda changed_plan, changed: changed["rust"]["components"][0].update(size=1))
rejects(lambda changed_plan, changed: changed["validators"][0].update(asset="unknown.tar.gz"))
rejects(lambda changed_plan, changed: changed["validators"][0].update(official_sha256="0" * 64))
rejects(lambda changed_plan, changed: changed["validators"][0].update(extracted_binary_sha256="0" * 64))


def rejects_call(call):
    try:
        call()
    except SystemExit:
        return
    raise AssertionError("malformed path or archive member was accepted")


def rejects_plan(mutator):
    changed = copy.deepcopy(plan)
    mutator(changed)
    rejects_call(lambda: materializer.validate_plan_paths(changed))


for invalid_path in ("/absolute", "a//b", "a/./b", "a/../b", "../escape", "a/"):
    rejects_call(lambda invalid_path=invalid_path: materializer.safe_rel_path(invalid_path))
for invalid_destination in ("relative/path", "//double/root", "/../escape", "/a//b", "/"):
    rejects_call(lambda invalid_destination=invalid_destination: materializer.destination_rel_path(invalid_destination))
rejects_call(lambda: materializer.validate_symlink_target("bin/sh", "../../../outside"))
rejects_plan(lambda changed: changed["rootfs_files"][0].update(destination="/../escape"))
rejects_plan(lambda changed: changed["rootfs_devices"][0].update(path="/dev//null"))
rejects_plan(lambda changed: changed["rootfs_symlinks"][0].update(target="../../../outside"))

device_parent = materializer.os.open("/", materializer.os.O_RDONLY | materializer.os.O_DIRECTORY)
device_paths = []
writer = object.__new__(materializer.TreeWriter)
writer.parentfd = lambda rel: (device_paths.append(rel) or device_parent, "null")
with patch.object(materializer.os, "mknod"), patch.object(materializer.os, "chown"), patch.object(materializer.os, "chmod"):
    writer.null_device()
assert device_paths == ["dev/null"]

rust_description = next(iter(materializer.RUST_ARCHIVES.values()))
archive_root = rust_description["prefix"].split("/", 1)[0]
root_member = tarfile.TarInfo(archive_root)
root_member.type = tarfile.DIRTYPE
assert materializer.classify_rust_member(root_member, rust_description)[0] == "ignored-root"
component_member = tarfile.TarInfo(rust_description["prefix"][:-1])
component_member.type = tarfile.DIRTYPE
assert materializer.classify_rust_member(component_member, rust_description)[0] == "ignored-component-root"
metadata_member = tarfile.TarInfo(archive_root + "/install.sh")
assert materializer.classify_rust_member(metadata_member, rust_description)[0] == "ignored-metadata"
selected_member = tarfile.TarInfo(rust_description["prefix"] + "bin/cargo")
assert materializer.classify_rust_member(selected_member, rust_description) == ("file", "bin/cargo")

for malformed_name in (
    archive_root + "/cargo/../escape",
    archive_root + "//cargo/file",
    archive_root + "/./cargo/file",
    archive_root + "/unlisted-metadata",
):
    malformed = tarfile.TarInfo(malformed_name)
    rejects_call(lambda malformed=malformed: materializer.classify_rust_member(malformed, rust_description))
malformed_link = tarfile.TarInfo(rust_description["prefix"] + "link")
malformed_link.type = tarfile.SYMTYPE
rejects_call(lambda: materializer.classify_rust_member(malformed_link, rust_description))
malformed_validator = tarfile.TarInfo("actionlint")
malformed_validator.type = tarfile.SYMTYPE
rejects_call(lambda: materializer.validate_validator_members([malformed_validator], next(iter(materializer.VALIDATOR_ARCHIVES.values()))))

print(
    "PASS actual plan/provenance/assets; path counts=",
    json.dumps(path_counts, sort_keys=True),
    "; eight cross-schema, fifteen plan/path, six member fixtures rejected",
    sep="",
)

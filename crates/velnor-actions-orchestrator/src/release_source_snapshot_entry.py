"""Unregistered read-only source producer; output authority comes from its owner."""
import hashlib
import os
from pathlib import Path


def source_snapshot_main():
    # The eventual compiled owner supplies this fixed capability. Ambient output
    # paths and arbitrary environment dictionaries grant no output authority.
    output = globals().get("write_source_snapshot_outputs")
    require(callable(output), "source_snapshot_output_unqualified")
    authenticate_original_source_origin()
    approved = policy()
    source = source_tree(approved)
    raw = serialize_source_snapshot(source)
    require(type(raw) is bytes, "source_snapshot_bytes")
    digest = hashlib.sha256(raw).hexdigest()
    directory = Path("release-source-snapshot")
    directory.mkdir(mode=0o700, exist_ok=False)
    destination = directory / "snapshot.zip"
    with destination.open("xb") as archive:
        archive.write(raw)
        archive.flush()
        os.fsync(archive.fileno())
    destination.chmod(0o400)
    output(digest, source.source_sha, source.tree_sha)


if __name__ == "__main__":
    source_snapshot_main()

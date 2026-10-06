"""Fixed output writer, embedded only in the output-capable snapshot owner."""
import os
import re
import stat


def _source_snapshot_output_writer():
    # The compiled helper's closed runner capability supplies this channel.
    # This module alone is not a capability or an independently admitted helper.
    path = os.environ.get("GITHUB_OUTPUT")
    require(type(path) is str and os.path.isabs(path), "source_snapshot_output_path")
    descriptor = os.open(path, os.O_WRONLY | os.O_APPEND | os.O_NOFOLLOW | os.O_NONBLOCK)
    if not stat.S_ISREG(os.fstat(descriptor).st_mode):
        os.close(descriptor)
        raise ReconcileError("source_snapshot_output_kind")
    emitted = False

    def write(snapshot_sha256, source_commit_sha, source_tree_sha):
        nonlocal emitted
        require(not emitted, "source_snapshot_output_repeated")
        emitted = True
        with os.fdopen(descriptor, "ab") as output:
            require(type(snapshot_sha256) is str and
                    re.fullmatch(r"[0-9a-f]{64}", snapshot_sha256), "source_snapshot_output_digest")
            require(all(type(value) is str and re.fullmatch(r"[0-9a-f]{40}", value)
                        for value in (source_commit_sha, source_tree_sha)),
                    "source_snapshot_output_sha")
            raw = ("source-snapshot-blob-sha256=" + snapshot_sha256 + "\n" +
                   "source-commit-sha=" + source_commit_sha + "\n" +
                   "source-tree-sha=" + source_tree_sha + "\n").encode("ascii")
            output.write(raw)
            output.flush()
            os.fsync(output.fileno())

    return write


write_source_snapshot_outputs = _source_snapshot_output_writer()

# Keep runner records before job cleanup

The host stores allowed fields from GitHub Runner and Worker trace files in
`<host-state>/completion-diagnostics/<sha256(connection-id)>`.
Each capture binding identifies the journal, repository connection, intent,
reservation, operation, and topology. It also identifies the engine, Docker
root digest, Runner container, Docker-in-Docker container, and `RunnerHome`
volume. The cleanup claim generation can change during recovery. The capture
binding stays the same.

Before cleanup removes the Runner container, it confirms the container has
stopped. The host reads traces from the mounted `RunnerHome` volume while the
Docker-in-Docker container remains available. Before the writer publishes a
record, the host saves the expected record digest, byte count, and whether
trace data is present. After restart, the host accepts only a private file
that matches the saved identity, digest, byte count, and trace status. The
host starts a new attempt only after it confirms that the previous writer is
idle. A file mismatch, unknown execution ID, unavailable trace data, or
uncertain capture keeps the reservation charged.

This cleanup path permits at most three capture attempts for one binding.
Each attempt submits at most one record write. An interrupted write can leave
one temporary file with at most 512 KiB of record data. The maximum temporary
record data is 1.5 MiB per binding. A published record contains at most 512
KiB. A stored capture state contains at most 8 KiB. Each repository connection
uses one fixed temporary file for capture state. This file contains at most
8 KiB. A partial or mismatched state temporary file blocks state writes for
the connection. Keep the file until you verify its ownership and state.
These limits exclude file system metadata and unused space within allocated blocks.
Data limits for one binding do not set a maximum for total retained disk usage.
The host does not automatically remove published records or temporary files
left by a crash.

Replace `/path/to/host-state` with the configured host state directory. Then
run these read-only checks:

```sh
df -kP "/path/to/host-state"
du -sk "/path/to/host-state/completion-diagnostics"
```

Record the reported free space and directory size, with their units, during cleanup incidents.
Keep files if ownership, identity, completion
state, or temporary file status is uncertain. Do not remove an unchecked file
to make cleanup continue. Use the host's approved disk management procedure
to correct disk space problems. Then retry cleanup. The host resumes from the
saved state and checks the existing record.

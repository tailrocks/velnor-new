# Rust runner job sandbox

`velnor-runner sandbox --profile job` runs one job inside the velnor machine. The host starts it as root through `orb`; it is not a container engine.

## What the job gets

- A constructed root: `pivot_root` into a fresh tmpfs. Read-only binds for `/usr`, `/etc`, the name-resolution file, the `bin`, `tools` and `crates` stores, each `--store-read-only` scope and `/srv/velnor/gen`. Read-write binds only for the job's work directory, which the job sees at the fixed path `/work` (so dependency-info paths and target generations match across jobs), its `home`, its `tmp` and its stage directory. The `work` directory of the generations is masked, so a job sees no other job's directory. Fresh `/proc`, a minimal `/dev`, an empty `/run`.
- Its own network namespace `velnor-job-<id>`, entered with `nsenter --net` before the other namespaces. See Network.
- Its own mount, pid, IPC and UTS namespaces. `sandbox-apply` is pid 1 of the pid namespace: it stays root, keeps the parent-death signal, reaps orphans and runs the command as an unprivileged user as pid 2. Killing the sandbox parent therefore ends every process of the job.
- No new privileges, an empty capability bounding and inheritable set, a Landlock abstract-socket scope (a hard requirement: the sandbox refuses to start on a kernel without it), a hard `RLIMIT_FSIZE` of 16 GiB.
- A seccomp filter that refuses: `AF_VSOCK` sockets (`EAFNOSUPPORT`), user namespaces (`EPERM`), `clone3`, `io_uring_*` and the keyring calls (`ENOSYS`), `setxattr`, `lsetxattr`, `fsetxattr` and `setxattrat` (`ENOTSUP`, which tools read as a filesystem without extended attributes), and every ioctl whose command has the btrfs group `(cmd & 0xff00) == 0x9400` (`ENOTTY`), except `FICLONE`, `FICLONERANGE` and `FIDEDUPERANGE`, so reflink copies work and subvolumes, snapshots and subvolume flags cannot be created or changed. A job that fails on `ENOTTY` gets an exact allow entry; the mask stays.
- A per-job cgroup `/sys/fs/cgroup/velnor/job-<id>` with `pids.max` 16384 and `memory.max` at 90% of the machine's memory divided by `--max-jobs` (the host passes its `max_jobs`), with the whole group killed on an out-of-memory event. Shared memory is bounded by `memory.max`.

## Delivery over standard input

With `--stdin-frames` the host streams two frames over `orb` standard input to the root sandbox parent, and no file on the Mac, on a share or in the machine's disk holds either:

- each frame is a 4-byte big-endian length followed by that many bytes, at most 64 KiB (65536 bytes) and never empty; a frame outside that bound, a short read or a missing frame fails the start before any directory exists, and the host refuses a JIT or a facts record over the bound before it spawns anything. The JIT configuration therefore has a bound of 64 KiB, a decision of the project owner, below the 131,040 bytes the protocol allows;
- the first frame is the JIT configuration. The parent writes it into a pipe that it closes; the command reads it to end of file and replaces its standard input with `/dev/null`. The job cannot read it through `/proc/1/fd/0`;
- the second frame is the trusted facts. The parent validates them and writes them to `/run/velnor/jobs/<id>/trusted.json` (mode 0600, created new, root only) in a directory on a tmpfs that it refuses to use otherwise. The job's `/run` is a fresh tmpfs, so the file is out of its reach. The parent removes the directory when the job ends and `sandbox-cleanup` removes it as well.

## Resource breaches

A breach (any of the kills below) ends the job with exit code 125 and the `velnor:` message on standard error. The parent then writes the typed record `{"breach":"<kind>"}` (`disk`, `foreign_device`, `machine_full` or `processes`) to `/run/velnor/jobs/<id>/breach.json`. The host reads it only after an exit 125 and treats the job as breached, which is a job failure and not a runner failure: the runner-health breaker (three consecutive failed exits close admission) counts only exits without a record.

## Network

The root sandbox parent builds the namespace before the job starts: a veth pair `vj<n>` (machine side) and `eth0` (job side) with a /31 from 198.18.0.0/16, the slot taken under a lock on `/var/lib/velnor/locks/net.lock`, `lo` up, and the default route through the machine side. At every start it loads one atomic nft table, `inet velnor_job`, in the machine's root namespace:

- input from `vj*` is rejected, so a job cannot reach the machine;
- forward drops IPv6 from `vj*`, and drops anything new towards `vj*`, so jobs cannot reach each other;
- port 53 is allowed only to the one IPv4 resolver of `/opt/orbstack-guest/etc/resolv.conf`; the table is not built, and the job does not start, unless the file names exactly one;
- the private, link-local, loopback, carrier-grade, documentation, benchmark and multicast ranges are rejected, which closes the Mac's bridge, the LAN, the gateway and other machines;
- only traffic leaving through the single default-route device is allowed, and return traffic must be established or related;
- masquerade is applied on the way out.

Teardown deletes `vj<n>`, then the namespace. The startup sweep removes every `vj*` and `velnor-job-*`. A resolver or default-device change after an OrbStack update fails closed. The machine and its trusted host and broker commands can still reach the Mac from the root namespace, and DNS goes to the Mac's resolver; both are accepted.

## Disk accounting

The machine's disk is capped outside the VM by the `--disk` qgroup, and that cap is the hard wall. Two checks stand in front of it:

- the floor. Every 100 ms the parent reads the live counter of the generations subvolume's btrfs qgroup under `/sys/fs/btrfs/<fsid>/qgroups/0_<subvolid>`: free space is `max_referenced - (referenced + rsv_data)`. `rsv_data` is live (a `fallocate` is charged at once) while `referenced` alone lags up to the 30 s commit. Never `rsv_meta_*`, never `statvfs`, which reports the whole disk of the Mac. Below 4 GiB, or when the counter cannot be read, the job is killed with `velnor: the machine's disk is nearly full`;
- the per-job limit, a fairness check. Every second a separate thread walks the work directory and the stage directory with `openat`, one descriptor per level, at most 1024 levels, and sums `st_blocks` once per `(dev, ino)`. Only the bytes the walk can see count: over the limit (64 GiB by default) the job is killed with `velnor: job used <n> GiB of disk; the limit is <limit> GiB`. A walk that runs past one second, goes deeper than 1024 levels or meets an entry it cannot open is a lower bound, never a kill on its own. The thread never blocks signals or exit.
- any entry whose `st_dev` differs from the root of its directory (a nested subvolume or a mount) kills the job with `velnor: job created a mount or a subvolume under its directories`.

The per-job limit therefore counts the visible data of the job's directories only. Data hidden from the walk (an unlinked file in a private descriptor table, a tree below 1024 levels, a descriptor in flight over a socket) is bounded by the floor and by the wall, not by the limit. Extended attributes are refused by seccomp.

Reaching `pids.max` kills the job with `velnor: job started more than 16384 processes`.

## Cleanup

`velnor-runner sandbox-cleanup --reservation <id>` kills the cgroup, waits until it is empty, removes it, tears down the network and removes the job's directories and its run directory. `--all` does it for every job of the machine. The host runs the first one as the cleanup proof of a job and the second one at startup; exit code 0 is the proof.

The removal never recurses and never walks a path. It opens the top directory once and flattens the tree with constant descriptors: it lifts each directory into the top with `renameat2` and `RENAME_NOREPLACE` under a fresh name, unlinks the files, and removes the emptied directory. Every step leaves a tree the next run continues from, so a crashed or repeated run converges; a tree a million levels deep takes about twenty seconds with four descriptors. An entry on another device (a nested subvolume) or a directory that stays non-empty ends with exit code 70, which the host reports as `JobDirsWedged`.

## Live lane

`bun scripts/velnor-sandbox-live.ts <machine>` runs the live tests on a throwaway isolated machine. It installs the pinned mise (checked against a recorded sha256), then the pinned Rust, mbx and cargo-nextest through mise, verifies that `cargo` resolves to the mise command wrapper and runs everything through mbx. They need OrbStack, so they run on a developer's Mac and not on GitHub-hosted runners.

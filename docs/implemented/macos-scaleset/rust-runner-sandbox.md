# Rust runner job sandbox

`velnor-runner sandbox --profile job` runs one job inside the velnor machine. The host starts it as root through `orb`; it is not a container engine.

## What the job gets

- A constructed root: `pivot_root` into a fresh tmpfs. Read-only binds for `/usr`, `/etc`, the name-resolution file, the `bin`, `tools` and `crates` stores, each `--store-read-only` scope and `/srv/velnor/gen`. Read-write binds only for the job's work directory, its `home`, its `tmp` and its stage directory. Fresh `/proc`, a minimal `/dev`, an empty `/run`.
- Its own network namespace `velnor-job-<id>`, entered with `nsenter --net` before the other namespaces. See Network.
- Its own mount, pid, IPC and UTS namespaces. `sandbox-apply` is pid 1 of the pid namespace: it stays root, keeps the parent-death signal, reaps orphans and runs the command as an unprivileged user as pid 2. Killing the sandbox parent therefore ends every process of the job.
- No new privileges, an empty capability bounding and inheritable set, a Landlock abstract-socket scope (a hard requirement: the sandbox refuses to start on a kernel without it), a hard `RLIMIT_FSIZE` of 16 GiB.
- A seccomp filter that refuses: `AF_VSOCK` sockets (`EAFNOSUPPORT`), user namespaces (`EPERM`), `clone3`, `io_uring_*` and the keyring calls (`ENOSYS`), `setxattr`, `lsetxattr`, `fsetxattr` and `setxattrat` (`ENOTSUP`, which tools read as a filesystem without extended attributes), and every ioctl whose command has the btrfs group `(cmd & 0xff00) == 0x9400` (`ENOTTY`), except `FICLONE`, `FICLONERANGE` and `FIDEDUPERANGE`, so reflink copies work and subvolumes, snapshots and subvolume flags cannot be created or changed. A job that fails on `ENOTTY` gets an exact allow entry; the mask stays.
- A per-job cgroup `/sys/fs/cgroup/velnor/job-<id>` with `pids.max` 16384 and `memory.max` at 90% of the machine's memory, with the whole group killed on an out-of-memory event.
- The JIT configuration reaches the command only through a pipe that the sandbox parent fills and closes. The runner reads it to end of file and replaces its standard input with `/dev/null`. No file holds it.

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

The machine's disk is capped outside the VM, and there is no filesystem quota. The sandbox parent keeps a watchdog instead:

- every second a separate thread walks the work directory and the stage directory with `openat`, no recursion and no path length limit, and sums `st_blocks` once per `(dev, ino)`; it adds files that are unlinked but still open or mapped by a process of the cgroup;
- over the limit (64 GiB by default), the job is killed with `velnor: job used <n> GiB of disk; the limit is <limit> GiB`;
- any entry the walk cannot read or open counts as over the limit and kills the job with the same kind of message;
- any entry whose `st_dev` differs from the root of its directory (a nested subvolume or a mount) kills the job with `velnor: job created a mount or a subvolume under its directories`;
- a walk that takes longer than one second kills the job with `velnor: job has too many files to account for its disk use`;
- the accounting covers file data only. Extended attributes are refused by seccomp and descriptors in flight over a socket are not counted, so what the walk cannot see is bounded by the floor: when the free space of the machine drops below 4 GiB, read from the btrfs qgroup of the generations subvolume (`max_referenced - referenced` under `/sys/fs/btrfs/<fsid>/qgroups/0_<subvolid>`, never from `statvfs`, which reports the whole disk of the Mac); a machine without a `max_referenced` kills the job, the job is killed with `velnor: the machine's disk is nearly full`;
- reaching `pids.max` kills the job with `velnor: job started more than 16384 processes`.

All of these end with exit code 125.

## Cleanup

`velnor-runner sandbox-cleanup --reservation <id>` kills the cgroup, waits until it is empty, removes it and removes the job's directories with an iterative, directory-descriptor-relative walk. `--all` does it for every job of the machine and also removes every `vj*` device and `velnor-job-*` namespace. A nested subvolume under a job directory is removed like any other directory. The trusted-facts file is opened `O_NOFOLLOW`, must be a regular file of at most 64 KiB and is read by the root parent before the sandbox is entered. The host runs the first one as the cleanup proof of a job and the second one at startup; exit code 0 is the proof.

## Live lane

`bun scripts/velnor-sandbox-live.ts <machine>` runs the live tests on a throwaway isolated machine. They need OrbStack, so they run on a developer's Mac and not on GitHub-hosted runners.

# Rust runner machine

The `rust-runner` launch mode runs every job in the OrbStack machine `velnor`. The host refuses any other shape and prints `orb delete velnor`; it never edits or recreates an existing machine.

## Shape

`ensure_machine` creates the machine as:

```text
orb create --isolated --isolate-network --disk <cap>G -a arm64 ubuntu velnor
```

The machine has no mount: nothing of the Mac's file system is shared with it, and nothing it writes lands outside its disk cap. It then checks the machine twice: what `orb list -f json` reports, and what is true inside the machine. The listing is configuration, and a configuration that is not applied yet looks like an applied one in it. Only the inside check proves what holds now, and it can prove the mounts, the sockets, the `mac` command and the disk cap. It cannot prove `isolate_network` or `forward_ssh_agent`: the host trusts those two flags as the listing reports them.

`orb list -f json` must report all of these:

- `config.isolated` and `config.isolate_network` are true, and `config.forward_ssh_agent` is false.
- `config.disk_limit_bytes` equals the cap.
- `config.mounts` is absent or empty.

Inside the machine, as root, on every call of `ensure_machine`:

- `fs.protected_hardlinks=1` and `fs.protected_symlinks=1`.
- `nft`, `ip` and `btrfs` exist. The host installs `nftables` and `btrfs-progs` when one is missing and refuses the machine when a tool is still missing (`btrfs` clears the read-only flag of a subvolume in the startup sweep).
- `/mnt/mac` does not exist, and `find /opt/orbstack-guest -type s` prints nothing.
- `/opt/orbstack-guest/bin/mac uname` fails.
- There is no mount of type `virtiofs`, `9p`, `fuse` or `fuse.*`. A machine created with a `--mount`, or one whose mount is still applied after its configuration dropped it, is refused.
- The btrfs qgroup of the subvolume that holds `/srv/velnor/gen` has `max_referenced` equal to the cap.

## What the isolation flags do

`--isolated` removes the channels from the machine to the Mac's control plane: no docker, ssh-agent or host-control sockets in `/opt/orbstack-guest/run`, no `/mnt/mac`, no listener on the Mac's `127.0.0.1` through `host.orb.internal`. The `mac`, `orb` and `open` commands still exist in the machine as stubs of `/opt/orbstack-guest/bin/macctl`, but they cannot reach the Mac: `mac uname` fails with `dial: no such file or directory`, and `orb list` prints the same message yet exits 0, so never read the exit status of these stubs as an answer. `--isolate-network` and `--disk` have the effects described below.

## What they do not do

- `--isolate-network` does not stop the machine from reaching the Mac's other addresses: services on the Mac's bridge interface and LAN, other OrbStack machines, and the IPv6 address of `host.orb.internal` stay reachable. Only the per-job network namespace and the nftables table of the sandbox close that, and a job is only as confined as that sandbox. Two residuals stay: DNS goes to the Mac's resolver, and the host's own commands and the broker run in the machine's root network namespace, so they can reach the Mac.
- The default user of an OrbStack machine has passwordless sudo, and its `~/.bash_profile` runs before every `orb -m` command in its shell. The host therefore never runs a command as that user (see [Identity](#identity)).
- `--disk` is a limit on one btrfs qgroup, not a filesystem quota (see [Disk cap](#disk-cap)).

## Disk cap

The cap is set in `host.toml`:

```toml
[host.machine_disk]
store_budget_gib = 100
job_limit_gib = 64
job_slack_gib = 8
```

The cap is `store_budget_gib + max_jobs * (job_limit_gib + job_slack_gib)`. With the defaults above and `max_jobs = 1` it is 172 GiB. A value that overflows, sets `job_limit_gib` to 0 or exceeds 16384 GiB fails the config parse.

OrbStack enforces `--disk` as `max_referenced` on the qgroup of the machine's subvolume (`/sys/fs/btrfs/<fsid>/qgroups/0_<subvolid>/max_referenced`). `ensure_machine` reads that value on every call and requires it to equal the cap, so a machine whose cap differs from the configuration, or has none, is refused.

What the qgroup counts is the data referenced by that subvolume. It does not count the data of a nested btrfs subvolume: any user can create one, and its qgroup has no limit. The cap therefore bounds the machine only together with the sandbox's own controls (the ioctl mask that refuses subvolume creation, the `st_dev` tripwire and the free-space floor of the sandbox). The sandbox is not part of the host, and this page makes no claim for it.

The host records `orbstack=<version> cap=<bytes>` in `/var/lib/velnor/cap-probe` inside the machine. When that record differs, it runs a probe that asks `fallocate` for one GiB more than the cap, on `/var/lib/velnor/.cap-probe` (outside every root a job sees). The probe and the removal of its file run in one root `sh -c`, so the file is gone whatever the result. The probe passes only when `fallocate` fails and its stderr (the host keeps the stderr of every guest command it reads) says `No space left on device` or `Disk quota exceeded`; any other failure (a missing `fallocate`, an unsupported call, an `orb` error) refuses the machine. It then rewrites the record. An OrbStack update, or a resized machine, therefore reruns the probe.

## Layout

All of it is on the machine's root btrfs, owned by root, and none of it is shared with the Mac:

| Path | Mode | Use |
|---|---|---|
| `/var/lib/velnor`, `bin`, `tools`, `crates`, `cache`, `git` | 0755 | Stores and the runner copy |
| `/var/lib/velnor/locks`, `aside`, `incoming`, `live` | 0700 | Never mounted into a sandbox |
| `/srv/velnor`, `gen`, `gen/work`, `gen/stage` | 0755 | Generations; the parents of the per-job `work/<reservation>` and `stage/<reservation>` dirs |

`/`, `/var`, `/var/lib` and `/srv` must be root-owned and not writable by group or other, or the host refuses the machine. The host lstats every store directory first. An entry that is not a directory (a symlink, a file) is refused with `MachineLayout` before any command changes anything: `install -d` would follow a link and change the mode of its target. A directory with the wrong mode is repaired; one owned by another user is refused by name (`StoreOwner`) and never re-owned. `/var/lib/velnor` and `/srv/velnor/gen` must both report `btrfs` from `stat -f -c %T`.

## Identity

Every host-to-guest command is built in one place and runs as root, with a cleared environment:

```text
orb -m velnor -u root /usr/bin/env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin LC_ALL=C <argv>
```

Only `orb list`, `orb create` and `orb version` run without `-m`.

The job user `velnor-job` is created when it is missing (`<home>` is `/home`):

```text
useradd --system --user-group --no-create-home --home-dir <home>/velnor-job --shell /usr/sbin/nologin velnor-job
```

`ensure_machine` reads the account as root and returns a `GuestUser` (`uid()`, `gid()`, `home()`; the fields are private and only the parser builds one) only when all of these hold:

- the uid and the gid are not 0 and the gid is not 27 (the `sudo` group);
- its only group is its own (`id -G` is its gid, `id -Gn` is `velnor-job`), so it is in none of `sudo`, `adm` or `orbstack`;
- its shell is `/usr/sbin/nologin` and its home is an absolute path;
- the uid is its alone: `getent passwd <uid>` and the full `getent passwd` name only `velnor-job` for that uid;
- the gid is its alone: `getent group <gid>` names only `velnor-job` with no member, and no other group and no other account (as primary gid) uses it; this refuses a uid or gid shared with the default user or with `sudo`, which file permissions would treat as the same identity;
- `sudo -n -l -U velnor-job` reports `User velnor-job is not allowed to run sudo on <host>.` and nothing else, with status 0. The host asks sudo for the resolved policy instead of searching the sudoers files for a name, so `ALL ALL=...`, `#<uid>`, `%#<gid>` and group rules count. A missing `sudo`, an error or any other report refuses the account.

An existing user that fails one of these is refused, not edited.

## Runner binary

The host hashes its own `velnor-runner` (`VELNOR_RUNNER_BINARY`, default: next to the host executable) after opening it with `O_NOFOLLOW` and checking it is an owned regular file closed to group and other and without an ACL (the same `ls -lde` check as the host store). It then streams that opened file over `orb` stdin into `/var/lib/velnor/bin/.stage`, a root-only file, and checks its sha256 against the host digest. Only then does it `chmod 0755` the file and `mv -fT` it to `/var/lib/velnor/bin/velnor-runner`. `-T` treats the destination as a file, so a directory there makes `mv` fail instead of receiving the stage, and a symlink there is replaced, not followed. After the rename the host checks again that the installed path is a regular root-owned `0755` file with the expected digest, and removes it when it is not. A failed copy removes `.stage` and leaves the installed runner as it was. Nothing the machine can write outside its own disk is read as the runner. The digest a job records for the runner health is the digest of this verified copy in the machine, not of the host file.

## Host store

The host store (the `Shared/velnor` folder of the macOS `Users` directory by default) holds only `host.lock`, the lock that allows one controller per store. It must be owned by the host user, closed to group and other and free of ACLs. It is not mounted into the machine. The startup sweep removes the trees earlier layouts left there: `bin`, `crates`, `tools`, `git`, `gen`, `cache`, `records`, `work`, `jit` and `trusted`. A symlink among them is unlinked, never followed.

## Launch

`start_job` refuses with `LaunchClosed` and touches neither the host store nor the machine. A launch that runs the runner as the machine's root would let a job rewrite the installed runner and root's files, so the launch stays closed until the sandbox confines jobs. The JIT of a job never goes into a file: the launch passes it to the runner on stdin.

## Job directories

A job's work and stage dirs are inside the machine: the host removes `/srv/velnor/gen/work/<reservation>` and `/srv/velnor/gen/stage/<reservation>` inside the machine, as root, with `rm -rf`. It does not use `--one-file-system`, because that would leave a nested btrfs subvolume (which a job can create) and wedge every later cleanup. The removal checks that both paths are gone.

One root `sh -c` removes both paths and checks they are gone, and its exit status tells a failed removal (70) or a leftover path (71) apart from an `orb` failure, which is retried like any other held proof. When the removal fails or a path is still there afterwards, the host returns `JobDirsWedged` and quarantines the slot: it is not proven again, its runner stays registered, and it keeps its place among the `max_jobs` slots, so the machine admits one job fewer. Nothing retries it. Remove the directories by hand in the machine and restart the daemon; the startup sweep runs one root command that first clears the read-only flag of every btrfs subvolume under `work` and `stage` (`btrfs property set -ts <subvol> ro false`), then removes every entry and lists what is left. A leftover whose name is a reservation number is quarantined at startup like one at runtime: it holds a slot, the controller starts and no new job reuses its number. A leftover with any other name is not a reservation and fails the sweep with `JobDirsWedged`.

## Guest user and job limit

`RustRunnerLauncher::job_limit_gib` exposes `job_limit_gib` for the sandbox's disk limit.

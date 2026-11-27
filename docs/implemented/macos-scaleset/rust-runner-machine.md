# Rust runner machine

The `rust-runner` launch mode runs every job in the OrbStack machine `velnor`. The host refuses any other shape and prints `orb delete velnor`; it never edits or recreates an existing machine.

## Shape

`ensure_machine` creates the machine as:

```text
orb create --isolated --isolate-network --disk <cap>G --mount <host store>:/var/lib/velnor-host -a arm64 ubuntu velnor
```

It then checks the machine twice: what `orb list -f json` reports, and what is true inside the machine. A machine that is configured but not applied passes the first check and fails the second.

`orb list -f json` must report all of these:

- `config.isolated` and `config.isolate_network` are true, and `config.forward_ssh_agent` is false.
- `config.disk_limit_bytes` equals the cap.
- `config.mounts` holds exactly one mount: the host store at `/var/lib/velnor-host`.

Inside the machine, as root, on every call of `ensure_machine`:

- `fs.protected_hardlinks=1` and `fs.protected_symlinks=1`.
- `nft` and `ip` exist. The host installs `nftables` when `nft` is missing and refuses the machine when either tool is still missing.
- `/mnt/mac` does not exist, and `find /opt/orbstack-guest -type s` prints nothing.
- `/opt/orbstack-guest/bin/mac uname` fails.
- The mounts of type `virtiofs`, `9p`, `fuse` or `fuse.*` are exactly the share at `/var/lib/velnor-host`. The host compares the canonical path of the share with the canonical path `orb list` reports.
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

All of it is on the machine's root btrfs, owned by root, and none of it is on the share:

| Path | Mode | Use |
|---|---|---|
| `/var/lib/velnor`, `bin`, `tools`, `crates`, `cache`, `git` | 0755 | Stores and the runner copy |
| `/var/lib/velnor/locks`, `aside`, `incoming`, `live` | 0700 | Never mounted into a sandbox |
| `/srv/velnor`, `gen`, `gen/work`, `gen/stage` | 0755 | Generations; the parents of the per-job `work/<reservation>` and `stage/<reservation>` dirs |

`/`, `/var`, `/var/lib` and `/srv` must be root-owned and not writable by group or other, or the host refuses the machine. A store directory with the wrong mode is repaired; one owned by another user is refused by name (`StoreOwner`) and never re-owned. `/var/lib/velnor` and `/srv/velnor/gen` must both report `btrfs` from `stat -f -c %T`.

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

`ensure_machine` reads it as root (`getent passwd`, `id -G`, `id -Gn`) and returns a `GuestUser` (`uid()`, `gid()`, `home()`; the fields are private and only the parser builds one) only when all of these hold:

- the uid and the gid are not 0;
- its only group is its own (`id -G` is its gid, `id -Gn` is `velnor-job`), so it is in none of `sudo`, `adm` or `orbstack`;
- its shell is `/usr/sbin/nologin` and its home is an absolute path;
- no line of `/etc/sudoers` or `/etc/sudoers.d` names it.

An existing user that fails one of these is refused, not edited.

## Runner binary

The host hashes its own `velnor-runner` (`VELNOR_RUNNER_BINARY`, default: next to the host executable) after opening it with `O_NOFOLLOW` and checking it is an owned regular file closed to group and other. It then streams that opened file over `orb` stdin into `/var/lib/velnor/bin/.stage`, a root-only file, and checks its sha256 against the host digest. Only then does it `chmod 0755` the file and rename it to `/var/lib/velnor/bin/velnor-runner`. A failed copy removes `.stage` and leaves the installed runner as it was. The share holds no runner: nothing the machine can write is read as the runner.

## Share

The share at `/var/lib/velnor-host` holds `jit` (the host writes `<reservation>.jit` files that the runner of the job reads) and `trusted` (the trusted facts of a job, `trusted/<reservation>.json`). Both are `0700` for the host user.

The machine can write the share, so the host treats it as adversary-controlled. When `jit` or `trusted` has drifted, `ensure_machine` repairs it: a directory with group or other access gets mode `0700` through its descriptor, and a symlink or a file is removed and the directory is created again. A directory owned by another user is refused. Do not rely on file modes on the share from inside the machine.

## Job directories

A job's work and stage dirs are not on the share: the host removes `/srv/velnor/gen/work/<reservation>` and `/srv/velnor/gen/stage/<reservation>` inside the machine, as root, with `rm -rf`. It does not use `--one-file-system`, because that would leave a nested btrfs subvolume (which a job can create) and wedge every later cleanup. The removal checks that both paths are gone.

One root `sh -c` removes both paths and checks they are gone, and its exit status tells a failed removal (70) or a leftover path (71) apart from an `orb` failure, which is retried like any other held proof. When the removal fails or a path is still there afterwards, the host returns `JobDirsWedged` and quarantines the slot: it is not proven again, its runner stays registered, and it keeps its place among the `max_jobs` slots, so the machine admits one job fewer. Nothing retries it. Remove the directories by hand in the machine and restart the daemon; the startup sweep empties `work` and `stage` as root and fails with `JobDirsWedged` while it cannot.

## Guest user and job limit

`RustRunnerLauncher::job_limit_gib` exposes `job_limit_gib` for the sandbox's disk limit.

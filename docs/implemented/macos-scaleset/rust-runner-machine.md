# Rust runner machine

The `rust-runner` launch mode runs every job in the OrbStack machine `velnor`. The host refuses any other shape and prints `orb delete velnor`; it never edits or recreates an existing machine.

## Shape

`ensure_machine` creates the machine as:

```text
orb create --isolated --isolate-network --disk <cap>G --mount <host store>:/var/lib/velnor-host -a arm64 ubuntu velnor
```

It accepts a listed machine only when `orb list -f json` reports all of these:

- `config.isolated` and `config.isolate_network` are true, and `config.forward_ssh_agent` is false.
- `config.disk_limit_bytes` equals the cap.
- `config.mounts` holds exactly one mount: the host store at `/var/lib/velnor-host`.
- The guest has `fs.protected_hardlinks=1` and `fs.protected_symlinks=1`.

The isolation flags keep a job that escapes its sandbox away from the Mac: no `host.orb.internal` listener, no `mac` or `orb` commands, no docker, ssh-agent or host-control sockets, no `/mnt/mac`.

## Disk cap

The cap is a hard limit outside the machine. It is set in `host.toml`:

```toml
[host.machine_disk]
store_budget_gib = 100
job_limit_gib = 64
job_slack_gib = 8
```

The cap is `store_budget_gib + max_jobs * (job_limit_gib + job_slack_gib)`. With the defaults above and `max_jobs = 1` it is 172 GiB. A value that overflows, sets `job_limit_gib` to 0 or exceeds 16384 GiB fails the config parse.

The host records `orbstack=<version> cap=<bytes>` in `/var/lib/velnor/cap-probe` inside the machine. When that record differs, it runs a probe that asks for one GiB more than the cap with `fallocate`, on `/var/lib/velnor/.cap-probe` (outside every root a job sees), and requires the call to fail. It then rewrites the record. An OrbStack update, or a resized machine, therefore reruns the probe.

## Layout

All of it is on the machine's root btrfs, owned by root, and none of it is on the share:

| Path | Mode | Use |
|---|---|---|
| `/var/lib/velnor`, `bin`, `tools`, `crates`, `cache`, `git` | 0755 | Stores and the runner copy |
| `/var/lib/velnor/locks`, `aside`, `incoming`, `live` | 0700 | Never mounted into a sandbox |
| `/srv/velnor`, `gen`, `gen/work`, `gen/stage` | 0755 | Generations; the parents of the per-job `work/<reservation>` and `stage/<reservation>` dirs |

`/`, `/var`, `/var/lib` and `/srv` must be root-owned and not writable by group or other, or the host refuses the machine. `/var/lib/velnor` and `/srv/velnor/gen` must both report `btrfs` from `stat -f -c %T`.

## Runner binary

The host hashes its own `velnor-runner` (`VELNOR_RUNNER_BINARY`, default: next to the host executable). The share holds the copy the machine installs, and the machine accepts it only when the root-owned staged copy has the host's sha256. The share is writable from the machine, so its content is never the source of the digest.

## Share

The share at `/var/lib/velnor-host` holds `bin`, `jit` and `trusted` (the trusted facts of a job, `trusted/<reservation>.json`). `jit` and `trusted` are `0700` for the host user. Do not rely on file modes on the share from inside the machine. A job's work and stage dirs are not on the share: the host removes `/srv/velnor/gen/work/<reservation>` and `/srv/velnor/gen/stage/<reservation>` inside the machine, as root.

## Guest user and job limit

`ensure_machine` returns the machine's unprivileged user (uid, gid and home, read from the default user of the machine) and refuses a root user. `RustRunnerLauncher::job_limit_gib` exposes `job_limit_gib` for the sandbox's disk limit.

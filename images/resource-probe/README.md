# Resource probe image

This build context packages the `velnor-resource-probe` executable in a
`scratch` image for `linux/amd64`. The executable reads bounded kernel data.
It measures the read-only `/velnor/docker-root` mount through one open file
descriptor. It reports the mount ID, mount device, statx device, filesystem
type, filesystem ID, boot ID, byte counts, and raw inode counts. Btrfs
subvolumes can report a different statx device. The probe keeps both values and
checks the mount ID and filesystem type. It does not read files from the
Docker-root mount.

The executable reports guest `MemAvailable` and its own cgroup v2 values.
These cgroup values describe the probe only. They do not describe runner or
worker memory, CPU, process, or swap limits. The host must measure those limits.
The probe does not select a Docker root or storage pool policy. The host binds
the report to the selected engine and configured storage pools.

The version 2 JSON record preserves finite inode counts. It reports
`dynamic_btrfs` only when Btrfs reports zero for all raw inode counts. Unknown
inode policy must close storage admission. Btrfs byte counts do not prove a
hard storage quota.

The image has numeric user and group `65532:65532`, a fixed entrypoint, no
command, the exact environment emitted by the pinned image builder, and an OCI
revision label supplied by the exact-source image release workflow. The
executable reads no environment values; the host passes no environment
overrides. It has no shell, package manager, or runtime image dependency. The
release builder must inspect the saved image, smoke-run it
with its required restrictions, and derive the resource-probe manifest from
the image configuration and archive it actually built.

The image's presence does not qualify host-side admission, container options,
resource policy, production capacity, or consumer adoption. See
[`../../docs/proposed/resource-probe-artifact.md`](../../docs/proposed/resource-probe-artifact.md)
for the complete proposed artifact contract.

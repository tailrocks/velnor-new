# Resource probe image

This build context packages the `velnor-resource-probe` executable in a
`scratch` image for `linux/amd64`. The executable samples only fixed numeric
kernel data and `statvfs` metadata for `/velnor/docker-root`; it never reads or
enumerates files from that mount.

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

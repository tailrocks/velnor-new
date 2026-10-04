# Rust tool closure qualification

These are local macOS source/behavior probes, not fresh Linux hosted-runner
cache restore measurements. C02/T18 CI completion remains open.

## Qualified inputs

- Mise `2026.10.0` macOS arm64 executable SHA256
  `8d2007efdae0c2b64e3955257533e6ec17197bc2fdcbc5dd8f6847f92881deea`;
  downloaded bytes matched GitHub release API artifact digest.
- Rustup `1.29.1` macOS arm64 archive SHA256
  `ec1b9233e7f72990ecd8e62063fa7f6c3dfc2bec8e97f88bff165f9100ac696a`;
  downloaded bytes matched official archive checksum.
- Generator Linux x64 Rustup archive SHA256
  `dda7234360b7f578ca8b0ddcb80145646fa61a67c1720a5abc7051b35c9fcb71`;
  downloaded bytes independently matched official archive checksum.
- Rust selector: `rust[profile=minimal,components=clippy,rustfmt]@1.98.1`.

## Observed behavior

| Operation | Local elapsed seconds | Result |
| --- | ---: | --- |
| Exact Rustup initializer, default toolchain none | 0.265 | No compiler installation |
| Cold Mise Rust install | 42.642 | Five required components installed |
| Unchanged Mise install | 0.035 | Zero installed tools; no payload download |
| Third unchanged Mise install | 0.031 | Zero installed tools; no payload download |

Raw output and timings: [evidence directory](ci-performance-rust-tool-evidence).
These repeated runs reuse one isolated local closure; they do not prove archive
transfer, restore relocation, Linux timing, queue time, or MBX state persistence.
Sample size is one sequence; no percentile claim.

An earlier `2026.9.18` negative probe removed the owned `rustfmt` proxy.
Mise reported success without repairing it. Pinned backend availability checks
components/targets but does not prove all proxies. The new helper verifies the
exact owned manager digest, repairs seven fixed proxies as hardlinks to that
manager, and verifies rustc/Clippy/rustfmt. The source-matched local missing-proxy
repair returned `rustfmt 1.9.0-stable`; inode equality confirmed the proxy identity.

The cold log includes a Rustup self-update check. The final bootstrap explicitly
uses supported `rustup set auto-self-update disable`; a separate local probe
confirmed the setting. The cold timing therefore describes the recorded sequence
before that final configuration step, rather than final generated Linux setup.

## Independent review and negative cases

Independent agent `rustup_proxy_verify` checked pinned CLI/backend source,
initializer flags, proxy representation, and both helper scripts. Findings fixed:
conditional-shell guards, missing settings fast path, installer/settings symlink
writes, public arbitrary artifact bindings, and proxy symlink preservation.

Generated single-line helper scripts pass `bash -n`. Local shell fixtures proved
owned-root symlinks fail before download, an installer symlink leaves an external
sentinel unchanged, and missing settings triggers the exact archive acquisition.
These fixtures check shell control flow; they do not execute the Linux installer
on macOS. Linux integration and damaged toolchain repair still require CI proof.

## Sources

- [Pinned Mise Rust backend](https://github.com/jdx/mise/blob/v2026.10.0/src/plugins/core/rust.rs)
- [Pinned inline option grammar](https://github.com/jdx/mise/blob/v2026.10.0/src/toolset/tool_version_options.rs)
- [Rustup proxy implementation](https://github.com/rust-lang/rustup/blob/1.29.1/src/cli/self_update.rs)
- [Rustup initializer arguments](https://github.com/rust-lang/rustup/blob/1.29.1/src/cli/setup_mode.rs)
- [Rustup setting command](https://github.com/rust-lang/rustup/blob/1.29.1/src/cli/rustup_mode.rs)
- [Linux archive checksum](https://static.rust-lang.org/rustup/archive/1.29.1/x86_64-unknown-linux-gnu/rustup-init.sha256)

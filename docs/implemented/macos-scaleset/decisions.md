# macOS Scale Set decisions

| ID | Decision |
|---|---|
| D1 | Schema 2 is free. `VelnorConfig::SCHEMA` stays 1. Schema-1 inputs keep hosted-only generation. |
| D2 | Runner code is a nested workspace at `crates/velnor-runner`, not a root workspace member. Root `Cargo.toml` excludes that directory so Cargo does not treat it as a broken member. |
| D3 | Primary profile is `linux/amd64` even when the Docker VM is `aarch64`. Emulation is reported. ARM64 is never relabeled as the x64 baseline. |
| D4 | One host-wide capacity authority. `max_jobs` defaults to 1. `N>1` is a qualification case, not a second authority. |
| D5 | The official runner is not patched. No `/proc` spoof to bypass `AssertCompatibleOS`. |
| D6 | The journal uses the embedded `turso` crate, local file only. Not Turso Cloud and not the `libsql` crate. `0.8.1` may require Rust newer than the repo MSRV 1.98. If it fails to compile, pin `0.7.2` and record the compiler error. Do not invent a version. |
| D7 | Docker client is Bollard `0.21.1`, constructed with an explicit Unix socket. Never `connect_with_local_defaults`. |
| D8 | Comparison fails closed. `NOT_PROVEN` is not success. |
| D9 | `~/Library/Application Support/Velnor` from before `velnor-new` is legacy. The operator authorized wiping it. Fresh state is correct. |
| D10 | Protocol pin remains `actions/scaleset` `e6daac7` because that commit is still the default-branch HEAD. Wire tags, including Pascal-case `RunnerSetting`, come from that tree. |
| D11 | No Java adapter for ChainArgos. Pinned CI does not run Java or frontend tests. |
| D12 | Legacy runner sources are Apache-2.0 only. Do not relabel copied text as MIT. Prefer reimplementation of invariants over copying files. |
| D13 | `tokio =1.47.1` is not in the registry. Bollard 0.21.1 resolves `^1.47` to 1.49.0 as the oldest available non-yanked release. Pin `=1.49.0`. |
| D14 | Nested `deny.toml` allows `Zlib` because `foldhash` 0.2 (pulled by `turso` 0.8.1 via `tantivy`) declares that license. Path dependencies carry `version = "=0.1.0"` so cargo-deny does not treat them as wildcards. The generator allowlist is unchanged. |
| D15 | Native MBX jobs set `MBX_GC_AUTO=0` while action results are in flight, then run guarded `mbx clean` after their final workspace consumer and before the action post export. The clean preserves the action-owned object store and receipts. This ordering addresses active-result collection; it does not establish post-export disk headroom, which remains a warm-cache qualification gate. Untrusted pull requests stay read-only. |
| D16 | Do not retag `velnor-runner:ubuntu-26.04-2.337.0` or `velnor-dind:29.8.2` while a consumer attempt is running on those tags. A new container picks up the tag; an already-running container keeps the image it was created from. |
| D17 | Plan artifacts are named `velnor-plan-r<run_id>-a<run_attempt>`. `gh run rerun --failed` does not re-run Plan, so dependents look up an artifact that was never uploaded. A rerun of failed jobs must include Plan in the same attempt. |
| D18 | Owned Docker cleanup matches the full container id. `NetworkMode` is `container:<64-hex>`. A 12-character id is not ownership proof and must not authorize `docker rm`. |
| D19 | An expired scale-set session is only the specifically identified HTTP 400 on delete/reopen. Any other 400 remains a failure. |
| D20 | Generic DinD startup does not pull workload service images. It publishes the public socket only after bounded daemon readiness and fails nonzero if readiness fails. A workload-specific image requirement belongs to that workflow's declared inputs; RabbitMQ ARM64 component evidence in PR25 commit `f78a33973` does not authorize an unbounded universal startup pull. |
| D21 | `max_jobs` is the ceiling (still default 1, no fixed upper clamp). Each poll grows or shrinks the advertised capacity by one from host load, free memory, and free disk. A missing sample holds the previous count. The first advertisement starts at 1. Running jobs are not killed to shrink. |

# Generator configuration

The new Rust generator reads `.velnor/rust-ci.toml`.

## Optional crates mirror

The new generator accepts an explicit mirror in `.velnor/rust-ci.toml`. It is off when the table is absent. It applies only when the configured job runner exactly matches the mirror's runner label. Hosted runners and fork pull requests do not use the mirror.

```toml
[crates_mirror]
runner = "ubuntu-26.04-scale-set"
registry = "sparse+https://crates-mirror.velnor-system.svc.cluster.local/index/"
cainfo = "/opt/velnor/crates-mirror/ca-bundle.pem"
```

The registry must use sparse HTTPS and end with `/`. The CA bundle must have an absolute path and already exist on the runner. The runner's image supplies it. The generator requires a pinned `velnor-ci` release of at least `0.1.81` when this table is present, including when the helper is built from source. The released helper configures Cargo before that bootstrap build.

Each opted-in job runs `velnor-ci crates-mirror` before Cargo or tool installation. It writes `$CARGO_HOME/config.toml` (`$HOME/.cargo/config.toml` when `CARGO_HOME` is unset) with `source.crates-io.replace-with = "velnor-crates-mirror"`, the sparse registry and `http.cainfo`. It refuses an existing `config` or `config.toml` that it does not own. Repeated setup replaces the owned configuration. Cargo reports registry or CA failures as failures; it does not silently switch to public crates.io.

A Cargo wrapper on `PATH`, also exported as `CARGO`, passes the absolute configuration with Cargo's `--config` option. It runs a per-toolchain hard link of Cargo under a distinct executable name so Cargo preserves the wrapper in `CARGO` for build scripts and tests. When a hard link is unavailable, it installs a copy once. This keeps the mirror when a child command changes `CARGO_HOME` or runs from a temporary directory, and supports `cargo +toolchain`. Calling a toolchain's Cargo by an unrelated absolute path bypasses `PATH` and `CARGO` and is outside this invocation contract.

Writing the configuration only above the checkout misses temporary directories. Writing it only in Cargo's home misses a changed home. A configuration at `/.cargo/config.toml` would reach unrelated directories but needs filesystem-root access and changes unrelated jobs. The wrapper uses a per-job configuration instead.

The repository's mirror table names its scale-set runner. Its existing engine and maintenance workflows are separate from the new generator and do not consume this setting. Activating a generated Rust workflow requires the compatible helper release and a matching `runner` setting.

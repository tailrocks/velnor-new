# Explicit-token tool launch qualification

The pinned [runner script handler](https://github.com/actions/runner/blob/397b032cbf865e9c3ddfab89d533ec19325e1273/src/Runner.Worker/Handlers/ScriptHandler.cs)
applies the runner's prepended tool paths before starting a script. These paths
are part of the bootstrap context; an installed tool need not exist in a system
binary directory. [Rust 1.99.1 process execution](https://github.com/rust-lang/rust/blob/1.99.1/library/std/src/sys/process/unix/unix.rs)
uses the cleared child environment when resolving a bare executable after an
environment reset. Removing `PATH` can therefore prevent pinned Mise from
starting before any GitHub request occurs.

Explicit-token GitHub commands retain runner `PATH` and `HOME`. For Mise's
acquired cache, a nonempty `MISE_DATA_DIR` takes precedence; otherwise a
nonempty `XDG_DATA_HOME` selects its `mise` child. With neither set, Mise uses
its normal home-based default. The launcher never inherits `XDG_DATA_HOME`
itself. It clears all other ambient state, adds only the selected token,
disables installation, and rejects project configuration, hooks, proxies, and
alternate GitHub credentials. A real subprocess regression places Mise outside
default system paths and witnesses the cached pinned `gh` under the XDG-derived
root. This qualifies local execution behavior; live preview publication
remains a separate release acceptance step.

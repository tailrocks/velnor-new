# Explicit-token tool launch qualification

The pinned [runner script handler](https://github.com/actions/runner/blob/397b032cbf865e9c3ddfab89d533ec19325e1273/src/Runner.Worker/Handlers/ScriptHandler.cs)
applies the runner's prepended tool paths before starting a script. These paths
are part of the bootstrap context; an installed tool need not exist in a system
binary directory. [Rust 1.99.1 process execution](https://github.com/rust-lang/rust/blob/1.99.1/library/std/src/sys/process/unix/unix.rs)
uses the cleared child environment when resolving a bare executable after an
environment reset. Removing `PATH` can therefore prevent pinned Mise from
starting before any GitHub request occurs.

Explicit-token GitHub commands retain only runner `PATH`, `HOME`, and an
explicitly present `MISE_DATA_DIR` for tool discovery and the already acquired
cache. They still clear all other ambient state, add only the selected token,
disable installation, and reject project configuration, hooks, proxies, and
alternate GitHub credentials. A real subprocess regression places Mise outside
default system paths and checks both launch and the child environment. This
qualifies local execution behavior; live preview publication remains a separate
release acceptance step.

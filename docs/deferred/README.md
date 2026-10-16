# Deferred implementation

**Status:** Historical / Superseded. Active runner implementation lives in `crates/velnor-runner` governed by [macOS Scale Set runner](../proposed/macos-scaleset-runner.md).

V1 workflow generator entry gates and dogfooding have been satisfied. Runner support is actively implemented in `crates/velnor-runner` as a macOS Runner Scale Set controller using official runner containers and the Runner Scale Set protocol.

## Deferred specifications

- [Self-hosted execution roadmap and architecture](self-hosted-runner.md) — historical roadmap document; superseded by [macOS Scale Set runner](../proposed/macos-scaleset-runner.md).

The active normative standard uses official `actions/runner` containers supervised by `velnor-host` via the `actions/scaleset` protocol. The runner consumes V1 workflow/task contracts; it MUST NOT create a second workflow generator.

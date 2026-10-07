# Deferred implementation

**Status:** Deferred. No runner implementation is claimed by this document.

Velnor's proposed V1 product is the workflow generator. Self-hosted runner work MUST NOT start until every V1 entry gate in the [implementation plan](../content/../content/docs/proposed/implementation-plan.mdxx) passes and Velnor dogfoods its generated workflow on GitHub-hosted runners.

## Deferred specifications

- [Self-hosted execution roadmap and architecture](self-hosted-runner.md) — conflicting Scale Set, slot, architecture, and label clauses are superseded by the [macOS Scale Set runner](../proposed/macos-scaleset-runner.md)

The sequence is macOS host with official GitHub runner containers first, reuse on Debian second, and the genuine Velnor-native GitHub Actions protocol runtime as a later milestone. The later runner consumes V1 workflow/task contracts; it MUST NOT create a second workflow generator.

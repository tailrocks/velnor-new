# Credential-free preview progress encoding

The native preview producer can observe task output through the existing bounded
process runner. Observation is explicitly opt-in; credential-bearing commands
keep buffered output and never attach the observer. Each live chunk has a fixed
stream prefix, and source `#`, backslash, controls, and non-ASCII bytes are escaped.
The 256 KiB total live budget counts encoded bytes, prefixes, and one explicit
truncation marker; captured stdout/stderr limits and the original deadline remain
unchanged. These diagnostic lines are untrusted progress, not successful producer
or artifact-verification evidence.

This encoding follows the actual runner, rather than assuming a prefix disables
workflow commands: [ActionCommand.cs at runner v2.337.0](https://github.com/actions/runner/blob/v2.337.0/src/Runner.Common/ActionCommand.cs#L108-L140)
searches for legacy `##[` anywhere in a line; the modern parser requires leading
`::`. [ActionCommandManager.cs](https://github.com/actions/runner/blob/v2.337.0/src/Runner.Worker/ActionCommandManager.cs#L62-L63)
tries both parsers. Escaping every source `#` closes the legacy route, including
chunk boundaries; the fixed prefix and escaped newlines close the modern route.
The synchronous diagnostic sink must remain responsive: checks occur before and
after writes, but an in-flight OS write cannot be physically preempted.

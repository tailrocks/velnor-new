# Codex agent configuration

The trusted project configuration sets the primary coordinator and unassigned implementation work to `gpt-6-luna` at `max`. Named review agents use `gpt-6.1-sol` at `medium`. Each custom agent file sets both model and effort so the role does not depend on the parent agent's defaults.

| Role | Agent name | Model | Effort | Sandbox |
| --- | --- | --- | --- | --- |
| Primary session | project root settings | `gpt-6-luna` | `max` | inherited host/session setting |
| Default agent | `default` | `gpt-6-luna` | `max` | workspace write |
| Coordination | `coordinator` | `gpt-6-luna` | `max` | workspace write |
| Implementation | `worker`, `integration` | `gpt-6-luna` | `max` | workspace write |
| Repository research | `explorer` | `gpt-6-luna` | `max` | read only |
| Test creation and execution | `test` | `gpt-6-luna` | `max` | workspace write |
| Workstream ownership | `evidence`, `generator`, `build_efficiency`, `runner`, `seeds` | `gpt-6-luna` | `max` | workspace write |
| Release and deployment | `release`, `deployment` | `gpt-6-luna` | `max` | workspace write |
| Source review | `source_review` | `gpt-6.1-sol` | `medium` | read only |
| Design review | `design_review` | `gpt-6.1-sol` | `medium` | read only |
| Performance review | `performance_review` | `gpt-6.1-sol` | `medium` | read only |
| Acceptance review | `acceptance_review` | `gpt-6.1-sol` | `medium` | read only |

Use the matching agent name for every delegated task. The configured generic default is an implementation/coordinator role at Luna max; it does not infer that an unnamed task is a review. Send review tasks to the appropriate named Sol role. The coordinator assigns each mutable path to one owner; `integration` combines completed workstreams, while `release` and `deployment` each own one such task at a time. `.codex/config.toml` sets `agents.max_concurrent_threads_per_session = 15`, which caps concurrently open spawned-agent threads per session, excluding the primary thread. This is an upper bound on open threads, not a guarantee that 15 tasks will execute at once or that runtime capacity will always be available.

Project configuration applies only in trusted local Codex clients. It overrides user-level and selected profile settings, while CLI flags and `-c` overrides take precedence. A project `.codex/config.toml` cannot pin the machine-local provider; this host currently reports provider `openai`. From the repository root, when a launch must explicitly pin the provider, use `codex --strict-config -C . -c 'model_provider="openai"'`. Project configuration does not change a session that is already running. Open a new session in this repository to load the primary model and effort settings.

These files select model slugs and reasoning effort; they do not attest which backend snapshot served a particular response. The Codex CLI model catalog on 2026-10-04 listed `gpt-6-luna` with `max` support and `gpt-6.1-sol` with `medium` support; both entries had no upgrade target. The configured role values are not a fallback promise: if the exact model or effort cannot run, stop and report the mismatch.

The execution records below were checked in local rollout files under `~/.codex/sessions/2026/10/04/`. For subagents, the first column reflects the parent spawn request; for the primary thread it reflects the recorded turn selection. `turn_context.model` is Codex's selected model slug, not backend snapshot evidence. Provider values are observed runtime metadata, not project-level provider pins.

| Work role | Spawn request or turn selection | Rollout selected model / effort | Observed provider | Agent path and thread ID | Root session ID |
| --- | --- | --- | --- | --- | --- |
| Primary thread acting as independent reviewer | `gpt-6.1-sol` / `medium` | `gpt-6.1-sol` / `medium` | `openai` | `/root`, `01a10685-5bf3-7470-b7de-183c04711b16` | `01a10685-5bf3-7470-b7de-183c04711b16` |
| Configuration implementation and research | `gpt-6-luna` / `max` | `gpt-6-luna` / `max` | `openai` | `/root/configure_agents`, `01a10688-d438-7f31-8298-200f866b8f02` | `01a10685-5bf3-7470-b7de-183c04711b16` |
| Independent configuration review | `gpt-6.1-sol` / `medium` | `gpt-6.1-sol` / `medium` | `openai` | `/root/review_agent_config`, `01a10689-82f6-7490-a3b0-b77cc7ca4d7f` | `01a10685-5bf3-7470-b7de-183c04711b16` |

Review agents set `sandbox_mode = "read-only"` and their instructions prohibit edits. Codex can reapply live parent-session permission overrides when spawning a child, so preserve the no-edit rule even when runtime permissions differ. The primary session inherits its host/session sandbox because the project root does not set `sandbox_mode`.

For the installed CLI, strictly validate project configuration with `codex app-server --strict-config` from the repository root. CLI 0.160.0 does not expose an app-server `customAgents/list` RPC, so that endpoint cannot enumerate the named agents. The standalone `.codex/agents/*.toml` files follow the documented project-agent discovery layout; select roles by their `name` values and inspect rollout metadata after any future spawn that requires an exact runtime check.

The configuration follows the official [Codex subagent documentation](https://learn.chatgpt.com/docs/agent-configuration/subagents) and [config precedence](https://learn.chatgpt.com/docs/config-file/config-basic).

## Commit identity and trailers

The primary project session and every implementation or coordination role that prepares commits use the repository-local identity `Alexey Zhokhov <alexey@zhokhov.com>`. The project `developer_instructions` and each productive agent file require these exact trailers on every new commit, in this order. Configure the identity with `git config --local user.name "Alexey Zhokhov"` and `git config --local user.email "alexey@zhokhov.com"`:

```text
Co-authored-by: Codex <codex@openai.com>
Signed-off-by: Alexey Zhokhov <alexey@zhokhov.com>
```

Pass both trailers to `git commit` in that order, for example:

```sh
git commit \
  --trailer "Co-authored-by: Codex <codex@openai.com>" \
  --trailer "Signed-off-by: Alexey Zhokhov <alexey@zhokhov.com>" \
  -m "docs: explain repository agent roles"
```

Inspect the resulting message with `git show -s --format=%B HEAD`. Review-only agents must not edit or commit. Never rewrite a commit already pushed to a shared branch to correct its trailers; make a new follow-up commit and describe any historical metadata correction truthfully.

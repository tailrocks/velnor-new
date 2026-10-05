# State deliberately retained

- `/tmp/velnor-node2418-provenance-20261005/`: contains private GPG key material. Key directory secured to mode 700, related GPG agent stopped. Private material cannot be published to Git.
- `/root/velnor-consumer-scratch-20261005/`: original source inputs contain secret-pattern candidates, including a Sentry configuration. Safe source snapshots were published; flagged originals were excluded and the directory secured to mode 700. Exact paths are recorded in `extra-source.secrets0`. Not every flagged input was necessarily an actual secret.
- Existing user project checkouts and older/undated local branches remain because ownership of their creation was not established. Goal-related work and branch tips in those checkouts were nevertheless checkpointed/pushed.
- Existing system runner/controller/guardian services and unrelated repositories/processes were left in place; agents reported no ownership of them.

Earlier `exceptions` entries are an operational log. The 1,129 initially unclassified temporary Git directories were subsequently archived, committed, pushed, verified and removed. Unchanged Jackin clone tips were verified on upstream before removal. The PR59 clone's local commits were explicitly pushed after its original remote availability check returned 404. No unresolved temporary-Git-state exception remains from that list.

An automatic approval review rejected a recursive-rm cleanup command. No portion of that rejected command executed. Cleanup completed using scoped filesystem deletion limited to the verified path manifest, without following symlinks or crossing filesystems.

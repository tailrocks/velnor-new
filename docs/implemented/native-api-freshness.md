# Native GitHub metadata freshness

Native preview, stable and Homebrew authority reads request HTTP revalidation.
Each mutable JSON GET receives a unique URL nonce and `Cache-Control: no-cache,
no-store` plus `Pragma: no-cache`. The nonce combines a process identity,
nanosecond timestamp and checked sequence; it is not a secret. Query values and
pagination parameters retain their original bytes. Fragments, ambiguous keys
and caller-supplied reserved nonce keys are refused. Existing semantic route
allowlists validate the original route before the transport appends its nonce.

This applies to release/current-run/source reads, protected-branch/environment
and immutable-release settings, and native Tap writer/merge metadata. Binary
asset/artifact requests, mutation bodies, merge SHA conditions and credentials
are unchanged. It does not retrofit unrelated generated publishing shell scripts.

Repeated responses that agree do not by themselves prove current authority.
The focused cache-aware regression models source advancement and new reviews
on an unchanged PR head; old request shapes can replay the earlier observation.
Existing identity, signature, exact-head, report and policy checks still apply.

HTTP cache controls do not make GitHub's origin linearizable and do not close
all time-of-check/time-of-use races. The native publisher has no server-side
compare-and-swap against source main. A nonce is not a signed freshness token.
No hosted stale promotion or cache-rootcause attribution is claimed.

Primary semantics: [RFC9111 request directives](https://www.rfc-editor.org/rfc/rfc9111.html#name-request-directives).
GitHub CLI's [API implementation](https://github.com/cli/cli/blob/v2.102.1/pkg/cmd/api/api.go)
enables its local response cache only when `--cache` supplies a positive TTL.
The fix addresses HTTP response reuse beyond that optional local cache.

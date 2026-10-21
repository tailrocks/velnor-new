# Product version preparation for v0.1.35

This prepares workspace metadata for the native release policy request correction.
It also carries the subsequent main-branch DBT Postgres adapter fix: the generator
sets its experimental-adapter option from the indexed profile, without accepting
that engine option through the project environment table.
Jackin preview run 39534567581 built all six payloads and assembled its aggregate
successfully, then signer job 119015736624 rejected its generated request with
`package_signer_request_path_mismatch`. No replacement preview was published.

The generated workflow supplies a static package release policy. The previous
signer and current-run publisher still expected the earlier raw plan request.
The signer and current-run publisher now require the fixed policy request path,
canonical policy bytes equal to their static environment, and the existing
digest-pinned local policy/plan admission. Legacy raw-plan requests are rejected.
Direct Homebrew reads resolve the exact source policy independently, then retain
complete signature, attestation, published-byte, active-caller, and remote stable
tag-origin verification before preparation or mutation. They cannot acquire
signing authority. Native channel events without original IDs select this direct
read; retained dispatch or scheduled promotion requires both original IDs. Partial
IDs and cross-channel events fail closed. Ordinary stable dispatch is no longer
misclassified as retained recovery.

Regression coverage feeds actual rendered preview and stable signer/publisher
requests through their real local runtime entries. Restoring the old guards
produced the observed request-path failure; restoring the correction passed the
focused signer, Tap, continuation, and stable admission suite. Tampered policies,
plans, contexts, admission digests, request paths, and invalid Tap events remain
negative controls. Full repository gates and live publication remain separate.

The workspace version, 24 exact internal path requirements, and 14 local package
records in the root Cargo.lock advance together to 0.1.35. Registry package
records remain identical to the 0.1.34 base. The independently versioned runner,
historical release manifests, consumer pins, and tool configuration are unchanged.

This document records preparation, not a published release. Complete root and
nested runner checks, independent review, exact-source hosted CI, reviewed main
merge, and protected product release approval remain mandatory. Consumer adoption
and real signed preview publication must be verified after official publication.

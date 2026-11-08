# Reusable workflow attestation fixture

The subject and Sigstore bundle are release assets from
[velnor-actions v0.1.78](https://github.com/tailrocks/velnor-new/releases/tag/v0.1.78).
The signed run is `42339974423`, attempt 1, source
`ff0f3ce3e7f87c172dbfc3b4ff9640f82c5a6905`.

`gh-output.json` is the verification output from GitHub CLI 2.103.3:

```sh
gh attestation verify subject.json \
  --repo tailrocks/velnor-new --bundle bundle.jsonl \
  --signer-workflow tailrocks/velnor-new/.github/workflows/product-release-generator.yml \
  --signer-digest ff0f3ce3e7f87c172dbfc3b4ff9640f82c5a6905 \
  --source-digest ff0f3ce3e7f87c172dbfc3b4ff9640f82c5a6905 \
  --source-ref refs/heads/main --deny-self-hosted-runners --format json
```

The subject name retained in the attestation is
`velnor-actions-0.1.78-x86_64-unknown-linux-gnu.provenance.json`.

The caller is `product-release.yml`; the signer is the reusable
`product-release-generator.yml`. The certificate contains separate
`buildConfigURI`/`buildConfigDigest`, `buildSignerURI`/`buildSignerDigest`, and
`sourceRepositoryURI`/`sourceRepositoryDigest` fields.

This fixture proves the parser's field mapping against a genuine signed
reusable-workflow artifact. Its three revisions happen to be equal. It does not
prove premerge event capture, distinct-revision signing, a protected environment,
or candidate qualification. Tests that mutate captured output exercise structural
matching only; those mutations do not create valid signatures.

`check_untrusted_output_shape` checks bounded JSON and identity bindings. It does
not authenticate a signature or establish policy custody, and returns no verified
authority capability. A native cryptographic verifier must supply authenticated
output before these bindings can support admission.

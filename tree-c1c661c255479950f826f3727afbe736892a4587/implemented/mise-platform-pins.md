# Mise platform binary pins

The active Velnor candidate is Mise 2026.10.7. The release tag is
v2026.10.7, published 2026-10-09. GitHub's release API reports a verified tag
signature; the tag resolves to source commit
4599c53b4286ff101de876f8122feec0797b48b2 and tree
d500ae4a7705b79c90358ec6f153a7c7db80abdc.

On 2026-10-10, the release API SHA-256 for each raw executable was compared
with a downloaded binary. For Linux ARM64, macOS ARM64, and macOS x64, the
binary extracted from the official tar.gz asset also matched the raw executable
digest. The Linux x64 standalone artifact is the same executable bytes as the
raw asset.

| Target | Raw executable | SHA-256 | Bytes | Archive SHA-256 |
| --- | --- | --- | ---: | --- |
| x86_64-unknown-linux-gnu | [mise-v2026.10.7-linux-x64](https://github.com/jdx/mise/releases/download/v2026.10.7/mise-v2026.10.7-linux-x64) | 6eb1b890e90818417ca34c90dbbd47881917d5cd199f31b63b062ea9c6b18d85 | 161070528 | 3d31e3a53e8041b278ca999a4772d4575583098025384126e7aa8a31e0dac11c |
| aarch64-unknown-linux-gnu | [mise-v2026.10.7-linux-arm64](https://github.com/jdx/mise/releases/download/v2026.10.7/mise-v2026.10.7-linux-arm64) | c7108d85a32ba17e4747d31d4a42f39f0c134f16211e204e8ef0a49d4f518fe1 | 140138656 | 67bfc43bcc28de3a461b29fcf13a94a06019e4a1d3be37e4ff2c1347a576449b |
| aarch64-apple-darwin | [mise-v2026.10.7-macos-arm64](https://github.com/jdx/mise/releases/download/v2026.10.7/mise-v2026.10.7-macos-arm64) | f5171e341518a57e8c4e9280e28443e35d66212c51164c83be76794e0a78b014 | 127277984 | 5841e5ab5009b4c4dd2b641ddbfc6777cc1b9c9c0ffd375001e294540e9e9cc8 |
| x86_64-apple-darwin | [mise-v2026.10.7-macos-x64](https://github.com/jdx/mise/releases/download/v2026.10.7/mise-v2026.10.7-macos-x64) | c3355f0c56d1b9fe73a2ba30e034b4e483541b25b1ad812a87440abfaeec8baa | 155846176 | 809b60fb1f7f5b8794db9c2ac78e5dc40df2e2a2e91d75e7dc97da1ecdc2755e |

The measured target byte counts are the upper bounds used when the generator
projects the selected executable into an owned check home. Unsupported targets
fail generation. The exact source-builder archives and binary-member hashes
are recorded in the typed bootstrap catalog; these asset checks establish byte
identity, not successful installation or hosted runtime qualification.

## Hosted qualification on 2026-10-09: historical 10.6 evidence

The generated qualification.yml workflow's mise-pin mode checked out the
dispatch SHA and verified the version and executable SHA-256 for Mise
2026.10.6 on Linux x64 and macOS x64. Both required jobs succeeded for source
commit bbf2cb2df8380b173a0629d07cb862f4870588f8 in
[run 37995746931](https://github.com/tailrocks/velnor-new/actions/runs/37995746931):

| Target | Runner label | Job | Result |
| --- | --- | --- | --- |
| Linux x64 | ubuntu-26.04 | [114041252080](https://github.com/tailrocks/velnor-new/actions/runs/37995746931/job/114041252080) | Passed |
| macOS x64 | macos-15-intel | [114041252169](https://github.com/tailrocks/velnor-new/actions/runs/37995746931/job/114041252169) | Passed |

The run completed at 2026-10-09T21:50:11Z. This historical result does not
qualify Mise 2026.10.7, the generator release, or any additional runner family.
Hosted qualification for 2026.10.7 remains pending.

The target-specific digests are compiled into the generator. When hosted
qualification succeeds, record the qualifying run and target results, then
regenerate the workflow inputs that carry the qualification evidence. The
current official 2026.10.7 asset pins remain candidates until those hosted
checks pass.

# Preserved OpenTofu release provenance WIP

This snapshot keeps the existing official-release metadata and verification logs from the stopped task. It does not include the downloaded archive or extracted executable, and it makes no test or implementation claim.

- Original fixture: `/opt/velnor-task-fixtures/20261005/opentofu-1.13.1-official-provenance-20261005T1902Z/`
- Upstream tag peeled commit: `233700b795b6d2372f1c56ed57a4abaec9d36475`
- Release JSON SHA-256: `2dc56d588824b2c2b13b5e1a0d7ac1080cc02855dfe6c688aef9fc70bfa8fe0b`
- Checksum list / public certificate / signature SHA-256: `f4a03dd8613320c665848cb6e1962ee112d777c9c6a8a725586dcc341b1c7252` / `1c761e3fd8969b6bc76bbcb95ec1772e43e95b0a725fa6aa11274af128de5f6c` / `51c93f67e0d33df5b1b8ba005e9c871b5d13b3bc8d4dba1577a3ebcced82f70f`
- Archive SHA-256 (not copied): `378ada19d4bc70c43732004e8159be771b23b9a5afdf059e5f8a2b3fa2c70a69`
- Extracted `tofu` SHA-256 (not copied): `a325c8c2f6834575e440b03c2ba67f94256072754ac7787fea718be6f01fef6a`
- Rootless version output SHA-256: `4846df4a2a3c3f56fc539d5562a8e6b0955752ffaf9c774dc7737e618587a853`
- `files.sha256` SHA-256: `7ca1b70862b120d68965ac79b4ca2be9c3926c9db81aab3e20d525cf3c745967`

The metadata, public checksum certificate/signature, and existing text evidence were scanned for common credential/private-key patterns; none matched. Binary payloads and installation artifacts are intentionally excluded from this public snapshot.

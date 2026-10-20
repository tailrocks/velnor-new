The V32 `Journal::open` code created these database files.
The source was a Git archive of commit `021677c413a99e768f4f099b18805c8b21b90f87`, not the V33 source.
The producer used V32 journal and lifecycle APIs.
It did not change schema DDL or `user_version`.

- `empty-v32.db`: SHA-256 `e704c26eda632141f0b559099b64a6b8395a0ba6b448dfc96f251f79c7264174`; 1,101,824 bytes.
- `charged-physical-attempt-v32.db`: SHA-256 `84253e8da2ae20a1a4c0d878e6f8f0ee32964e2f5caf585404f9fb43a12e5dff`; 1,114,112 bytes. The file has one charged physical reservation, one acquired request authorization, and one Started lifecycle record.
The producer created them through journal APIs.

`journal/request_lifecycle/tests/v32_fixture_producer.rs` is the fixture producer harness (SHA-256: `6ee8b62a39ecec60597d79cd0aa24ada867554718e5acadc648e607aa2c67b78`).
Its Nextest run (`607431ac-5cd2-4290-913b-7faef4a9cc54`) passed 1/1.
The raw log SHA-256 is `429b45cb75d829173661af623c4fe0a8932258087d7b782fcc5672e2b5af6721`.

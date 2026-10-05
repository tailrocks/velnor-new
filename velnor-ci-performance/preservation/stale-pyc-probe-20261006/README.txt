PR88 stale timestamp/size pycache import-cache probe

Frozen Velnor commit: d3ed26f1a47aa67c8296d00607472f5c63116365
Scope: exact extractor and preflight Git blobs copied into isolated with-pyc and no-pyc fixtures. Benign synthetic bytecode prints one marker and raises; it does not extract or modify candidate outputs.

raw.log is the final run using the physical /private/tmp path. The earlier symlink-path-trial.log is retained as a setup attempt: macOS /tmp resolved through a symlink and the no-pyc control rejected the fixture path before reaching the missing .git marker; it is not the reported control result.
reproduce.py is the bounded Python harness. Each extractor subprocess has an 8-second timeout. It creates the same 10,240-byte candidate tar in each fixture and records command, stdout, stderr, return code, elapsed time, source hashes, cache header, Python identity, and output directory state.

No Docker, Cargo, network, credentials, repository writes, or candidate publication were used.

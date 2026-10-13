# PR25 aggregate image and work-volume proof

This receipt is bound to aggregate commit `1ece60d7e5ab73166730c1ad9f414b8e8c89577a`. It records local image, archive-tool, and work-volume checks only; it is not live Scale Set or Actions qualification. The older [3f2e35a7 receipt](runner-work-volume-pr25-aggregate-proof.md) remains a historical proof for its earlier context.

The exact 23 regular build inputs are listed in [runner-work-volume-pr25-final-context.sha256](runner-work-volume-pr25-final-context.sha256). All entries verified against the checkout. The newline-terminated manifest SHA-256 is `e267fee37fd6c7e72c535bfc819cb3cd56d30a93f08795e40bef44a52e15ba91`. The manifest excludes `.dockerignore`, which was absent. Build context, manifest, and checks are retained under `/root/.cache/velnor-pr25-final-proof-1ece60d`.

Both images were built for `linux/amd64` from Ubuntu 26.04 base digest `sha256:f144425ff09be612d6d9ad965196e9cdc23dae1f42110a8a11a3e9a8198759f7`, with Docker Engine 29.8.2 on x86_64 (engine ID `217d445c-e773-41da-b749-d0b32b0881e9`):

```sh
docker build --pull --progress=plain --platform linux/amd64 -t velnor-dind:pr25-final-1ece60d7 images/dind
docker build --pull --progress=plain --platform linux/amd64 -t velnor-runner:pr25-final-1ece60d7 images/runner/ubuntu-26.04
```

The inspected immutable DinD image is `sha256:cf25ee0752caf965af68d9ed078ae63f5dcad49b9ad9effb0be24cd173e1da23`; the runner image is `sha256:71363278e057e2f9c405a3ff766bb84e70d42ab0b740f7578b274d753e825929`. Both inspect as `linux/amd64`. DinD uses `/usr/local/bin/velnor-dind-entrypoint`; the runner uses UID 1000, workdir `/home/runner`, and `/usr/local/bin/velnor-runner-entrypoint`. The build logs are `dind-build.log` (SHA-256 `e7d6ae205dc1a2b6ad4e3364e80bb5df65a1e8a9e4aa710d882dec38815c12e5`) and `runner-build.log` (SHA-256 `ef6c083eec0131f23c7cc121cf794c40c5028679cf3335ef8143ae0ff11e0615`).

The runner image's installed `/usr/local/bin/velnor-tar` matches the source `tar-shim.sh` at SHA-256 `400270104f3376dcf33ac07aa953dd96f22aeba503c1bdec798cef5bbbf5b82e`. In the immutable runner image, the absolute/legacy-option suite passed 20/20 and the semantics suite passed 23/23. The logs are retained as `tar-absolute-test.log` (SHA-256 `2eb5f51a51ac6e1feb976166529057248ed3f8a5c695bc1392c2b2c927a180b2`) and `tar-semantics-test.log` (SHA-256 `15bedc310f9ca9bc880a8b2db0603aea2217c59282636d3f93e9d3ce44a65ae1`).

A unique named work volume was confirmed absent before creation. DinD's first mount initialized `/home/runner/_work` as `1000:1000 755`, then wrote a root-owned `0:0 644` marker. The runner's default UID 1000 read that marker and wrote a `1000:1000 644` marker. Running the actual runner entrypoint with a dummy JIT body and one socket-wait attempt exited 1 with `docker socket missing`. An instrumented `mktemp` observed a private `/tmp/velnor-jit.*` file at mode 600; the runner had no `/tmp` volume mount, and the temporary file was absent from the stopped container. All four named containers and the volume were confirmed absent after cleanup.

Raw commands, output, image inspections, context checks, preflight/postflight records, tar logs, and probe script are retained under `/root/.cache/velnor-pr25-final-proof-1ece60d/runtime-probe-20261005T183149Z-1323142`. The probe log SHA-256 is `1f5279308608e0654be343f0ae53e88aa932e56de127ff8493dcb01cd7b2385e`; the artifact hash index SHA-256 is `5a2941eadde9ee12a133b69612315b81ce2a97f5a8ddc2deffd5cce6f1201dd4`. Every entry in that index was independently rechecked. The proof establishes local image behavior, UID 1000 volume access, private JIT-file handling, tar behavior, and cleanup for this exact context; it does not establish native macOS Keychain behavior, a live Scale Set run, workload-cache behavior, or qualification completion.

# Ubuntu 26.04 job image

This job image runs Linux x86-64 job steps. It is separate from the Runner image.

The Dockerfile pins the Ubuntu image digest from `.velnor/runner.lock`. It adds
Bash, Git, curl, certificate authority (CA) certificates, Python 3, GNU tar,
gzip, unzip, XZ, Zstandard, and netcat. It adds Docker client 29.9.1,
Compose 5.6.0, and Buildx 0.38.0.
Before installation, `sha256sum` compares each artifact's SHA-256 with the
value in `.velnor/runner.lock`. It adds Node.js 26.12.0 for shell steps.

Velnor runs the Docker daemon in a separate image. This image contains the
Docker client, Compose and Buildx Docker CLI plugins. It does not contain the
Docker daemon, Runner, JIT data, resource probe, storage initializer, or
controller data.

The pinned official Runner release is 2.338.0. Source commit
`197ca027b90de199464dee3bb9ff198a1db60967` mounts its Runner tools at `/__e`.
JavaScript actions use the runtime from that mount. Node.js 26.12.0 is for shell
steps. It does not replace the action runtime.

`build-test.sh` compares the Dockerfile with `.velnor/runner.lock`. It does not
build an image. The current release workflow does not build or attest this
image.

A job image is a container image for the job container selected by a workflow.
A Runner image is a container image that contains the official Runner. A Docker
CLI plugin is a program that adds commands to the Docker client.

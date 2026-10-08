import assert from "node:assert/strict";
import test from "node:test";
import {
  assertRedisMountContract,
  assertRyukContainer,
  nodeVersionAtLeast,
  recordedVolumeNames,
  REDIS_DATA_TMPFS_OPTIONS,
  socketPathForEndpoint,
  validateEnvironment,
} from "./engine.mjs";
import { commonLabels, envFor, redisImage, runtime, ryukImage, sessionId } from "./test-support.mjs";

test("requires the resolved Testcontainers runtime engine floor", () => {
  assert.equal(nodeVersionAtLeast("20.18.0"), false);
  assert.equal(nodeVersionAtLeast("20.18.1"), true);
  assert.equal(nodeVersionAtLeast("20.19.0"), true);
  assert.equal(nodeVersionAtLeast("21.0.0"), true);
  assert.equal(nodeVersionAtLeast("20.18.1-rc.1"), false);
  assert.equal(nodeVersionAtLeast("not-a-version"), false);
});

test("accepts only the paired hosted and private DinD endpoint/socket contract", () => {
  for (const endpoint of ["unix:///var/run/docker.sock", "unix:///run/docker/docker.sock"]) {
    assert.equal(validateEnvironment(envFor(endpoint), runtime).endpoint, endpoint);
  }
  assert.equal(socketPathForEndpoint("unix:///var/run/docker.sock"), "/var/run/docker.sock");
  assert.equal(socketPathForEndpoint("unix:///run/docker/docker.sock"), "/run/docker/docker.sock");
  assert.throws(() => socketPathForEndpoint("tcp://docker:2375"), /unexpected Docker endpoint/);
});

test("fails closed on endpoint overrides, TLS, image drift, and workspace drift", () => {
  const checks = [
    (env) => { env.DOCKER_CONTEXT = "default"; },
    (env) => { env.TESTCONTAINERS_DOCKER_SOCKET_OVERRIDE = "/var/run/docker.sock"; },
    (env) => { env.DOCKER_TLS_VERIFY = "1"; },
    (env) => { env.RYUK_CONTAINER_IMAGE = "docker.io/testcontainers/ryuk:0.14.0"; },
    (env) => { env.GITHUB_WORKSPACE = "/tmp/other"; },
  ];
  for (const mutate of checks) {
    const env = envFor("unix:///run/docker/docker.sock");
    mutate(env);
    assert.throws(() => validateEnvironment(env, runtime));
  }
  assert.throws(() => validateEnvironment(envFor("unix:///run/docker/docker.sock"), {
    ...runtime, nodeVersion: "20.18.0",
  }), /Node 20\.18\.1/);
  assert.throws(() => validateEnvironment(envFor("unix:///run/docker/docker.sock"), {
    ...runtime, arch: "arm64",
  }), /amd64/);
});

test("accepts only the pinned Ryuk socket and session", () => {
  const socketPath = "/run/docker/docker.sock";
  const labels = { ...commonLabels(), "org.testcontainers.ryuk": "true", TESTCONTAINERS_RYUK_TEST_LABEL: "true" };
  const container = {
    image: ryukImage, state: "running", labels,
    binds: [`${socketPath}:/var/run/docker.sock:rw`],
    mounts: [{ type: "bind", source: socketPath, destination: "/var/run/docker.sock" }],
  };
  assert.equal(assertRyukContainer(container, { ryukImage, socketPath, sessionId }), sessionId);
  assert.throws(() => assertRyukContainer({ ...container, mounts: [] }, { ryukImage, socketPath, sessionId }));
  assert.throws(() => assertRyukContainer(container, { ryukImage, socketPath, sessionId: "other" }));
});

test("requires only Redis's bounded /data tmpfs and rejects other inspect mounts", () => {
  const container = { tmpfs: { "/data": REDIS_DATA_TMPFS_OPTIONS }, mounts: [] };
  assertRedisMountContract(container);
  for (const mounts of [[{ type: "volume", destination: "/data" }], [{ type: "bind", destination: "/extra" }]]) {
    assert.throws(() => assertRedisMountContract({ ...container, mounts }));
  }
  assert.throws(() => assertRedisMountContract({ ...container, tmpfs: {} }));
});

test("validates bounded recorded volume names", () => {
  assert.deepEqual(recordedVolumeNames(["owned-volume"]), ["owned-volume"]);
  assert.throws(() => recordedVolumeNames(["owned-volume", "owned-volume"]), /duplicates/);
  assert.throws(() => recordedVolumeNames(["../other"]), /invalid mounted volume name/);
});

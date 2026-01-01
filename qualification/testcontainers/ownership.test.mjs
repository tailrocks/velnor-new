import assert from "node:assert/strict";
import test from "node:test";
import {
  assertRedisContainer,
  mountedVolumeNames,
  parseContainerInspect,
  PROBE_LABEL,
  REDIS_DATA_TMPFS_OPTIONS,
  removeOwnedContainer,
  TESTCONTAINERS_VERSION,
} from "./engine.mjs";
import { commonLabels, probeId, redisImage, sessionId } from "./test-support.mjs";

test("accepts the pinned Redis session and rejects socket exposure or ownership drift", () => {
  const labels = { ...commonLabels(), [PROBE_LABEL]: probeId };
  const container = {
    image: redisImage,
    state: "running",
    labels,
    binds: [],
    tmpfs: { "/data": REDIS_DATA_TMPFS_OPTIONS },
    mounts: [],
  };
  assert.equal(assertRedisContainer(container, { redisImage, probeId }), sessionId);
  assert.throws(() => assertRedisContainer({ ...container, binds: ["/run/docker/docker.sock:/sock:rw"] }, {
    redisImage, probeId,
  }), /socket-isolation/);
  assert.throws(() => assertRedisContainer({ ...container, labels: { ...labels, [PROBE_LABEL]: "other" } }, {
    redisImage, probeId,
  }), /ownership/);
  assert.throws(() => assertRedisContainer({ ...container, labels: { ...labels, "org.testcontainers.version": "11.13.0" } }, {
    redisImage, probeId,
  }), /version mismatch/);
});

test("tracks only mounted volume names and removes one exact owned container", () => {
  const id = "a".repeat(64);
  const name = "anonymous-volume-1";
  assert.deepEqual(mountedVolumeNames({ mounts: [{ type: "volume", name, destination: "/data" }] }), [name]);
  const calls = [];
  const io = {
    containerIds: () => calls.length === 0 ? [id] : [],
    remove: (args) => calls.push(args),
    volumeExists: () => false,
  };
  removeOwnedContainer("unix:///run/docker/docker.sock", {
    id, mounts: [{ type: "volume", name, destination: "/data" }],
  }, io);
  assert.deepEqual(calls, [["rm", "-f", "-v", id]]);
});
test("inspect decoding retains tmpfs options and exact volume mount names", () => {
  const id = "c".repeat(64);
  const container = parseContainerInspect({
    Id: id, Config: { Image: redisImage, Labels: {} }, State: { Status: "created" },
    HostConfig: { Binds: [], Tmpfs: { "/data": REDIS_DATA_TMPFS_OPTIONS } },
    Mounts: [{ Type: "volume", Name: "anonymous-volume-2", Source: "/var/lib/docker/volumes/x", Destination: "/data" }],
  }, id);
  assert.equal(container.tmpfs["/data"], REDIS_DATA_TMPFS_OPTIONS);
  assert.deepEqual(container.mounts[0], { type: "volume", name: "anonymous-volume-2", source: "/var/lib/docker/volumes/x", destination: "/data" });
  assert.throws(() => parseContainerInspect({
    Id: id, Config: {}, State: {}, HostConfig: { Binds: [], Tmpfs: {} },
  }, id), /container metadata/);
});

test("keeps deletion scoped and fails for surviving volumes or ambiguous IDs", () => {
  const id = "a".repeat(64);
  const other = "b".repeat(64);
  let removals = 0;
  const volumeLeft = {
    containerIds: () => removals === 0 ? [id] : [],
    remove: () => { removals += 1; },
    volumeExists: () => true,
  };
  assert.throws(() => removeOwnedContainer("unix:///run/docker/docker.sock", {
    id, mounts: [{ type: "volume", name: "shared-volume", destination: "/data" }],
  }, volumeLeft), /volume mounted .* remains/);
  const ambiguous = { containerIds: () => [id, other], remove: () => { removals += 1; }, volumeExists: () => false };
  assert.throws(() => removeOwnedContainer("unix:///run/docker/docker.sock", { id, mounts: [] }, ambiguous), /ambiguous/);
  assert.equal(removals, 1);
});

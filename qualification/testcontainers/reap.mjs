import { randomUUID } from "node:crypto";
import { execFileSync } from "node:child_process";
import {
  assertRedisIdentity,
  assertRedisContainer,
  assertRyukIdentity,
  delay,
  environment,
  idsById,
  idsByLabel,
  inspect,
  mountedVolumeNames,
  PROBE_LABEL,
  recordedVolumeNames,
  removeOwnedContainer,
  SESSION_ID,
  volumeExists,
} from "./engine.mjs";

const runtime = environment();
const probeId = randomUUID();
if (idsByLabel(runtime.endpoint, "org.testcontainers.ryuk", "true").length !== 0) {
  throw new Error("isolated qualification daemon already has Ryuk");
}
if (idsByLabel(runtime.endpoint, PROBE_LABEL, probeId).length !== 0) {
  throw new Error("probe label collision");
}

let proof;
let ryukCleaned = false;
let redisVolumeNames = [];
try {
  const output = execFileSync(process.execPath, ["qualification/testcontainers/orphan.mjs"], {
    encoding: "utf8",
    timeout: 180_000,
    maxBuffer: 4_096,
    env: { ...process.env, VELNOR_TESTCONTAINERS_PROBE_ID: probeId },
  }).trim();
  proof = JSON.parse(output);
  if (!proof || !/^[a-f0-9]{64}$/.test(proof.redisId ?? "") ||
      !/^[a-f0-9]{64}$/.test(proof.ryukId ?? "") || proof.redisId === proof.ryukId ||
      !SESSION_ID.test(proof.sessionId ?? "") || !Array.isArray(proof.redisVolumeNames)) {
    throw new Error("Testcontainers returned an invalid ownership proof");
  }
  redisVolumeNames = recordedVolumeNames(proof.redisVolumeNames);

  const deadline = Date.now() + 120_000;
  while (Date.now() < deadline) {
    const remaining = idsByLabel(runtime.endpoint, PROBE_LABEL, probeId);
    if (remaining.length === 0) {
      ryukCleaned = idsById(runtime.endpoint, proof.redisId).length === 0 &&
        redisVolumeNames.every((name) => !volumeExists(runtime.endpoint, name));
      if (ryukCleaned) break;
      throw new Error("Redis or one of its recorded volumes remains after Ryuk cleanup");
    }
    if (remaining.length !== 1 || remaining[0] !== proof.redisId) {
      throw new Error("unexpected container appeared with the probe label");
    }
    const redis = inspect(runtime.endpoint, proof.redisId);
    assertRedisContainer(redis, {
      redisImage: runtime.redisImage,
      probeId,
      sessionId: proof.sessionId,
    });
    redisVolumeNames = mountedVolumeNames(redis);
    await delay(1_000);
  }
} finally {
  await cleanupOwnedResources(runtime, probeId, proof, redisVolumeNames);
}

if (!ryukCleaned) throw new Error("Ryuk did not remove the orphaned Redis container");
console.log("testcontainers-workspace-and-localhost-ok");
console.log("testcontainers-redis-ping-ok");
console.log("testcontainers-ryuk-pinned-socket-and-session-ok");
console.log("testcontainers-ryuk-orphan-cleanup-ok");

async function cleanupOwnedResources(runtime, probeId, proof, knownRedisVolumes) {
  const labeledRedis = idsByLabel(runtime.endpoint, PROBE_LABEL, probeId);
  if (proof && labeledRedis.some((id) => id !== proof.redisId)) {
    throw new Error("refusing to clean an unexpected probe-labeled container");
  }
  if (labeledRedis.length > 1) throw new Error("refusing ambiguous probe cleanup");

  const redisExists = proof ? idsById(runtime.endpoint, proof.redisId) : labeledRedis;
  if (redisExists.length > 1 || (proof && redisExists.some((id) => id !== proof.redisId))) {
    throw new Error("refusing ambiguous Redis identity cleanup");
  }
  let sessionId = proof?.sessionId;
  if (redisExists.length === 1) {
    const redisId = redisExists[0];
    const redis = inspect(runtime.endpoint, redisId);
    sessionId = assertRedisIdentity(redis, {
      redisImage: runtime.redisImage,
      probeId,
      sessionId,
      allowStopped: true,
    });
    const volumes = mountedVolumeNames(redis);
    knownRedisVolumes.push(...volumes.filter((name) => !knownRedisVolumes.includes(name)));
  }

  const reaperIds = idsByLabel(runtime.endpoint, "org.testcontainers.ryuk", "true");
  if (reaperIds.length > 1) throw new Error("refusing ambiguous Ryuk cleanup");
  let reaper;
  if (reaperIds.length === 1) {
    if (!sessionId) throw new Error("cannot prove the Ryuk session belongs to this probe");
    reaper = inspect(runtime.endpoint, reaperIds[0]);
    assertRyukIdentity(reaper, {
      ryukImage: runtime.ryukImage,
      socketPath: runtime.socketPath,
      sessionId,
      allowStopped: true,
    });
  }

  if (redisExists.length === 1) {
    const redis = inspect(runtime.endpoint, redisExists[0]);
    assertRedisIdentity(redis, {
      redisImage: runtime.redisImage,
      probeId,
      sessionId,
      allowStopped: true,
    });
    removeOwnedContainer(runtime.endpoint, redis);
  }
  if (reaper) removeOwnedContainer(runtime.endpoint, reaper);
  if (proof && idsById(runtime.endpoint, proof.redisId).length !== 0) {
    throw new Error("the exact Redis container remains after cleanup");
  }
  if (proof && idsById(runtime.endpoint, proof.ryukId).length !== 0) {
    throw new Error("the exact Ryuk container remains after cleanup");
  }
  if (knownRedisVolumes.some((name) => volumeExists(runtime.endpoint, name))) {
    throw new Error("a Redis volume remains after owned cleanup");
  }
  if (idsByLabel(runtime.endpoint, PROBE_LABEL, probeId).length !== 0) {
    throw new Error("probe-labeled Redis remains after owned cleanup");
  }
  if (idsByLabel(runtime.endpoint, "org.testcontainers.ryuk", "true").length !== 0) {
    throw new Error("Ryuk remains after owned cleanup");
  }
}

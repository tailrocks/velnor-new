import { GenericContainer, Wait } from "testcontainers";
import {
  assertRedisContainer,
  assertRyukContainer,
  assertRedisIdentity,
  environment,
  idsByLabel,
  inspect,
  mountedVolumeNames,
  pingRedis,
  REDIS_DATA_TMPFS_OPTIONS,
  PROBE_LABEL,
  removeOwnedContainer,
  SESSION_ID,
} from "./engine.mjs";

const runtime = environment();
const probeId = process.env.VELNOR_TESTCONTAINERS_PROBE_ID;
if (!SESSION_ID.test(probeId ?? "")) throw new Error("probe identity is invalid");
if (idsByLabel(runtime.endpoint, PROBE_LABEL, probeId).length !== 0) {
  throw new Error("probe label collision");
}
if (idsByLabel(runtime.endpoint, "org.testcontainers.ryuk", "true").length !== 0) {
  throw new Error("isolated qualification daemon already has Ryuk");
}

let redis;
let leaveForRyuk = false;
try {
  redis = await new GenericContainer(runtime.redisImage)
    .withLabels({ [PROBE_LABEL]: probeId })
    .withExposedPorts(6379)
    .withTmpFs({ "/data": REDIS_DATA_TMPFS_OPTIONS })
    .withStartupTimeout(90_000)
    .withWaitStrategy(Wait.forListeningPorts())
    .start();
  const host = redis.getHost();
  const port = redis.getMappedPort(6379);
  if (host !== "localhost" || !Number.isInteger(port) || port < 1 || port > 65535) {
    throw new Error("Testcontainers host/port discovery mismatch");
  }
  await pingRedis(host, port);

  const service = inspect(runtime.endpoint, redis.getId());
  const sessionId = assertRedisContainer(service, {
    redisImage: runtime.redisImage,
    probeId,
  });
  const redisVolumeNames = mountedVolumeNames(service);
  const serviceIds = idsByLabel(runtime.endpoint, PROBE_LABEL, probeId);
  if (serviceIds.length !== 1 || serviceIds[0] !== service.id) {
    throw new Error("unexpected containers carry the unique probe label");
  }

  const reaperIds = idsByLabel(runtime.endpoint, "org.testcontainers.ryuk", "true");
  if (reaperIds.length !== 1) throw new Error("expected one isolated Ryuk container");
  const reaper = inspect(runtime.endpoint, reaperIds[0]);
  assertRyukContainer(reaper, {
    ryukImage: runtime.ryukImage,
    socketPath: runtime.socketPath,
    sessionId,
  });

  leaveForRyuk = true;
  process.stdout.write(`${JSON.stringify({ redisId: service.id, ryukId: reaper.id, sessionId, redisVolumeNames })}\n`, (error) => {
    if (error) process.exit(1);
    process.exit(0);
  });
} finally {
  if (redis && !leaveForRyuk) {
    const owned = inspect(runtime.endpoint, redis.getId());
    assertRedisIdentity(owned, { redisImage: runtime.redisImage, probeId, allowStopped: true });
    removeOwnedContainer(runtime.endpoint, owned);
  }
}

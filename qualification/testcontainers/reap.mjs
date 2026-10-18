import { execFileSync } from "node:child_process";
import { GenericContainer, Wait } from "testcontainers";

const redis = await new GenericContainer(
  "redis:8.10.2-alpine@sha256:2d3814be5e9b06a30a0be54770b7e12052e7e79ec85271aefd34875c1f393b23",
)
  .withExposedPorts(6379)
  // v12 prefers image healthchecks; this qualification explicitly tests port readiness.
  .withWaitStrategy(Wait.forListeningPorts())
  .start();

let ryuk = false;
try {
  const listed = execFileSync("docker", ["ps"], { encoding: "utf8" });
  ryuk = listed.includes("ryuk");
} finally {
  await redis.stop();
}

if (!ryuk) {
  process.exit(1);
}

console.log("testcontainers-ok");
console.log("ryuk-seen");

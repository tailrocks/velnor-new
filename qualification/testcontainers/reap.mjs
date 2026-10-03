import { execFileSync } from "node:child_process";
import { GenericContainer } from "testcontainers";

const redis = await new GenericContainer("redis:7-alpine")
  .withExposedPorts(6379)
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

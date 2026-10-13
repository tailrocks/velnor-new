import { socketPathForEndpoint, TESTCONTAINERS_VERSION } from "./engine.mjs";

export const redisImage = "docker.io/library/redis@sha256:ca0acbb137c1dc3339c8b147a58fd6f42775d4599327b50e7b116c23de501af2";
export const ryukImage = "docker.io/testcontainers/ryuk@sha256:f0456560ea5b4acdbed0da0efc33b5f9dd6bc1e59f2337106826dcb5b0b0e981";
export const probeId = "1f8420d4-a067-44f8-95e6-91a7e98e820d";
export const sessionId = "5a88e6d0-61c4-42d0-897a-fdf67acb401a";
export const runtime = { nodeVersion: "24.20.0", arch: "x64", cwd: "/work/repo" };

export function envFor(endpoint) {
  return {
    DOCKER_HOST: endpoint,
    DOCKER_CONTEXT: "",
    TESTCONTAINERS_DOCKER_SOCKET_OVERRIDE: socketPathForEndpoint(endpoint),
    TESTCONTAINERS_HOST_OVERRIDE: "localhost",
    TESTCONTAINERS_RYUK_DISABLED: "false",
    TESTCONTAINERS_RYUK_TEST_LABEL: "true",
    RYUK_CONTAINER_IMAGE: ryukImage,
    VELNOR_TESTCONTAINERS_REDIS_IMAGE: redisImage,
    GITHUB_WORKSPACE: "/work/repo",
  };
}

export function commonLabels() {
  return {
    "org.testcontainers": "true",
    "org.testcontainers.lang": "node",
    "org.testcontainers.version": TESTCONTAINERS_VERSION,
    "org.testcontainers.session-id": sessionId,
  };
}

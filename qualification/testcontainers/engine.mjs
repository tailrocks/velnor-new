import { execFileSync } from "node:child_process";
import net from "node:net";
import path from "node:path";

export const PROBE_LABEL = "io.velnor.qualification.testcontainers-probe";
export const CONTAINER_ID = /^[a-f0-9]{64}$/;
export const SESSION_ID = /^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/;
export const TESTCONTAINERS_VERSION = "11.14.0";
export const REDIS_DATA_TMPFS_OPTIONS = "rw,size=67108864";

const REDIS_IMAGE = /^docker\.io\/library\/redis@sha256:[a-f0-9]{64}$/;
const RYUK_IMAGE = /^docker\.io\/testcontainers\/ryuk@sha256:[a-f0-9]{64}$/;

export function nodeVersionAtLeast(version, minimum = [20, 18, 1]) {
  const parsed = /^(\d+)\.(\d+)\.(\d+)$/.exec(version ?? "");
  if (!parsed) return false;
  const actual = parsed.slice(1).map(Number);
  for (let index = 0; index < minimum.length; index += 1) {
    if (actual[index] !== minimum[index]) return actual[index] > minimum[index];
  }
  return true;
}

export function socketPathForEndpoint(endpoint) {
  if (endpoint === "unix:///var/run/docker.sock") return "/var/run/docker.sock";
  if (endpoint === "unix:///run/docker/docker.sock") return "/run/docker/docker.sock";
  throw new Error("unexpected Docker endpoint");
}

export function validateEnvironment(env, runtime) {
  const endpoint = env.DOCKER_HOST;
  const socketPath = socketPathForEndpoint(endpoint);
  if (env.DOCKER_CONTEXT !== "") throw new Error("Docker context override is not allowed");
  if (env.TESTCONTAINERS_DOCKER_SOCKET_OVERRIDE !== socketPath) {
    throw new Error("Testcontainers socket path does not match the Docker endpoint");
  }
  if (env.TESTCONTAINERS_HOST_OVERRIDE !== "localhost") {
    throw new Error("Testcontainers host override mismatch");
  }
  if (env.TESTCONTAINERS_RYUK_DISABLED !== "false" || env.TESTCONTAINERS_RYUK_TEST_LABEL !== "true") {
    throw new Error("the isolated Ryuk cleanup contract is not enabled");
  }
  if (!REDIS_IMAGE.test(env.VELNOR_TESTCONTAINERS_REDIS_IMAGE ?? "")) {
    throw new Error("Redis image is not pinned by digest");
  }
  if (!RYUK_IMAGE.test(env.RYUK_CONTAINER_IMAGE ?? "")) {
    throw new Error("Ryuk image is not pinned by digest");
  }
  if (["DOCKER_TLS", "DOCKER_TLS_VERIFY", "DOCKER_CERT_PATH"].some((key) => env[key])) {
    throw new Error("TLS Docker settings are incompatible with the pinned Unix socket");
  }
  if (!nodeVersionAtLeast(runtime.nodeVersion)) throw new Error("Node 20.18.1 or newer is required");
  if (runtime.arch !== "x64") throw new Error("pinned Testcontainers images require amd64");
  const workspace = env.GITHUB_WORKSPACE;
  if (!workspace || path.resolve(workspace) !== path.resolve(runtime.cwd)) {
    throw new Error("Testcontainers must run from the checked-out workspace");
  }
  return {
    endpoint,
    socketPath,
    redisImage: env.VELNOR_TESTCONTAINERS_REDIS_IMAGE,
    ryukImage: env.RYUK_CONTAINER_IMAGE,
  };
}

export function environment() {
  return validateEnvironment(process.env, {
    nodeVersion: process.versions.node,
    arch: process.arch,
    cwd: process.cwd(),
  });
}

export function assertTestcontainersLabels(container, expectedSessionId) {
  const labels = container.labels;
  if (labels["org.testcontainers"] !== "true" || labels["org.testcontainers.lang"] !== "node") {
    throw new Error("container is missing Testcontainers ownership labels");
  }
  if (labels["org.testcontainers.version"] !== TESTCONTAINERS_VERSION) {
    throw new Error("container Testcontainers version mismatch");
  }
  const sessionId = labels["org.testcontainers.session-id"];
  if (!SESSION_ID.test(sessionId ?? "") || (expectedSessionId && sessionId !== expectedSessionId)) {
    throw new Error("container Testcontainers session identity mismatch");
  }
  return sessionId;
}

export function assertRedisContainer(container, { redisImage, probeId, sessionId, allowStopped = false }) {
  const resolvedSessionId = assertRedisIdentity(container, { redisImage, probeId, sessionId, allowStopped });
  assertRedisMountContract(container);
  return resolvedSessionId;
}

export function assertRedisIdentity(container, { redisImage, probeId, sessionId, allowStopped = false }) {
  const states = allowStopped ? ["running", "created", "exited"] : ["running"];
  if (container.image !== redisImage || !states.includes(container.state)) {
    throw new Error("pinned Redis container identity mismatch");
  }
  if (container.labels[PROBE_LABEL] !== probeId || container.binds.length !== 0) {
    throw new Error("Redis ownership or socket-isolation mismatch");
  }
  return assertTestcontainersLabels(container, sessionId);
}

export function assertRedisMountContract(container) {
  const tmpfs = container.tmpfs;
  const mounts = container.mounts;
  if (!tmpfs || Object.keys(tmpfs).length !== 1 || tmpfs["/data"] !== REDIS_DATA_TMPFS_OPTIONS ||
      !Array.isArray(mounts) || mounts.length !== 0) {
    throw new Error("Redis /data tmpfs or mount ownership mismatch");
  }
}

export function assertRyukContainer(container, { ryukImage, socketPath, sessionId, allowStopped = false }) {
  const resolvedSessionId = assertRyukIdentity(container, { ryukImage, socketPath, sessionId, allowStopped });
  if (container.mounts.length !== 1 || container.mounts[0].type !== "bind" ||
      container.mounts[0].destination !== "/var/run/docker.sock") {
    throw new Error("Ryuk inspect mounts do not match its exact socket bind");
  }
  return resolvedSessionId;
}

export function assertRyukIdentity(container, { ryukImage, socketPath, sessionId, allowStopped = false }) {
  const states = allowStopped ? ["running", "created", "exited"] : ["running"];
  if (container.image !== ryukImage || !states.includes(container.state)) {
    throw new Error("pinned Ryuk container identity mismatch");
  }
  if (container.labels["org.testcontainers.ryuk"] !== "true" ||
      container.labels.TESTCONTAINERS_RYUK_TEST_LABEL !== "true") {
    throw new Error("Ryuk test identity labels are missing");
  }
  if (container.binds.length !== 1 ||
      container.binds[0] !== `${socketPath}:/var/run/docker.sock:rw`) {
    throw new Error("Ryuk does not have the exact provider socket bind");
  }
  return assertTestcontainersLabels(container, sessionId);
}

export function docker(endpoint, args) {
  socketPathForEndpoint(endpoint);
  const childEnv = { ...process.env, DOCKER_HOST: endpoint, DOCKER_CONTEXT: "" };
  delete childEnv.DOCKER_TLS;
  delete childEnv.DOCKER_TLS_VERIFY;
  delete childEnv.DOCKER_CERT_PATH;
  return execFileSync("docker", ["--host", endpoint, ...args], {
    encoding: "utf8",
    timeout: 8_000,
    maxBuffer: 64 * 1024,
    stdio: ["ignore", "pipe", "pipe"],
    env: childEnv,
  });
}

function containerIds(output) {
  const ids = output.trim().split(/\r?\n/).filter(Boolean);
  if (ids.some((id) => !CONTAINER_ID.test(id)) || new Set(ids).size !== ids.length) {
    throw new Error("Docker returned malformed or duplicate container IDs");
  }
  return ids;
}

export function inspect(endpoint, id) {
  if (!CONTAINER_ID.test(id)) throw new Error("Docker returned a malformed container ID");
  const raw = JSON.parse(docker(endpoint, ["inspect", "--format", "{{json .}}", id]));
  return parseContainerInspect(raw, id);
}

export function parseContainerInspect(raw, id) {
  if (!CONTAINER_ID.test(id)) throw new Error("Docker returned a malformed container ID");
  if (!raw || raw.Id !== id || !raw.Config || !raw.State || !raw.HostConfig) {
    throw new Error("unexpected Docker inspect response");
  }
  const labels = raw.Config.Labels ?? {};
  const binds = raw.HostConfig.Binds ?? [];
  const tmpfs = raw.HostConfig.Tmpfs ?? {};
  const rawMounts = raw.Mounts;
  if (typeof labels !== "object" || Array.isArray(labels) || !Array.isArray(binds) ||
      typeof tmpfs !== "object" || Array.isArray(tmpfs) || !Array.isArray(rawMounts)) {
    throw new Error("unexpected Docker container metadata");
  }
  const mounts = rawMounts.map((mount) => {
    if (!mount || typeof mount !== "object" || typeof mount.Type !== "string" ||
        typeof mount.Destination !== "string" || !mount.Destination.startsWith("/")) {
      throw new Error("unexpected Docker mount metadata");
    }
    if (mount.Type === "volume" && !validVolumeName(mount.Name)) {
      throw new Error("Docker returned an invalid mounted volume name");
    }
    return {
      type: mount.Type,
      name: mount.Name ?? "",
      source: mount.Source ?? "",
      destination: mount.Destination,
    };
  });
  return { id: raw.Id, image: raw.Config.Image, state: raw.State.Status, labels, binds, tmpfs, mounts };
}

export function mountedVolumeNames(container) {
  if (!Array.isArray(container.mounts)) throw new Error("Docker mount inventory is incomplete");
  return [...new Set(container.mounts.filter((mount) => mount.type === "volume").map((mount) => {
    if (!validVolumeName(mount.name)) throw new Error("Docker returned an invalid mounted volume name");
    return mount.name;
  }))];
}

export function recordedVolumeNames(names) {
  if (!Array.isArray(names) || names.length > 64) {
    throw new Error("recorded Docker volume inventory is invalid");
  }
  const normalized = mountedVolumeNames({ mounts: names.map((name) => ({ type: "volume", name })) });
  if (normalized.length !== names.length) throw new Error("recorded Docker volume inventory has duplicates");
  return normalized;
}

export function removeOwnedContainer(endpoint, container, io = removalIo(endpoint)) {
  socketPathForEndpoint(endpoint);
  if (!CONTAINER_ID.test(container.id ?? "")) throw new Error("refusing cleanup without an exact container ID");
  const mountedVolumes = mountedVolumeNames(container);
  const before = io.containerIds(container.id);
  if (before.length !== 1 || before[0] !== container.id) {
    throw new Error("refusing cleanup with an ambiguous container ID");
  }
  // Docker's -v removes anonymous volumes attached to this exact container; named
  // volumes are left alone and detected by the terminal presence check below.
  try {
    io.remove(["rm", "-f", "-v", container.id]);
  } catch (error) {
    if (io.containerIds(container.id).length !== 0) throw error;
  }
  if (io.containerIds(container.id).length !== 0) {
    throw new Error("owned container remains after exact removal");
  }
  if (mountedVolumes.some((name) => io.volumeExists(name))) {
    throw new Error("a volume mounted by the removed owned container remains");
  }
}

function removalIo(endpoint) {
  return {
    containerIds: (id) => idsById(endpoint, id),
    remove: (args) => docker(endpoint, args),
    volumeExists: (name) => volumeNames(endpoint, name).includes(name),
  };
}

function volumeNames(endpoint, name) {
  if (!validVolumeName(name)) throw new Error("invalid Docker volume name");
  const output = docker(endpoint, ["volume", "ls", "-q", "--filter", `name=${name}`]);
  const names = output.trim().split(/\r?\n/).filter(Boolean);
  if (names.some((candidate) => !validVolumeName(candidate))) {
    throw new Error("Docker returned an invalid volume inventory");
  }
  return names;
}

export function volumeExists(endpoint, name) {
  return volumeNames(endpoint, name).includes(name);
}

function validVolumeName(name) {
  return typeof name === "string" && /^[A-Za-z0-9][A-Za-z0-9_.-]{0,127}$/.test(name);
}

export function idsByLabel(endpoint, label, value) {
  if (!/^[A-Za-z0-9_.-]+$/.test(label) || /[\r\n]/.test(value)) {
    throw new Error("invalid Docker label query");
  }
  return containerIds(docker(endpoint, ["ps", "-aq", "--no-trunc", "--filter", `label=${label}=${value}`]));
}

export function idsById(endpoint, id) {
  if (!CONTAINER_ID.test(id)) throw new Error("Docker returned a malformed container ID");
  return containerIds(docker(endpoint, ["ps", "-aq", "--no-trunc", "--filter", `id=${id}`]));
}

export function pingRedis(host, port) {
  return new Promise((resolve, reject) => {
    let response = "";
    const socket = net.createConnection({ host, port, family: 4 });
    const fail = (error) => { socket.destroy(); reject(error); };
    socket.setTimeout(3_000, () => fail(new Error("Redis PING timed out")));
    socket.once("error", fail);
    socket.once("connect", () => socket.write("PING\r\n"));
    socket.on("data", (chunk) => {
      response += chunk.toString("utf8");
      if (response.length > 128) return fail(new Error("Redis response exceeded limit"));
      if (response.includes("\r\n")) {
        socket.end();
        if (response === "+PONG\r\n") resolve();
        else reject(new Error("Redis PING response mismatch"));
      }
    });
  });
}

export const delay = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds));

import { randomBytes } from "node:crypto";
import { lstat, readFile, readdir, realpath } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { pathToFileURL, fileURLToPath } from "node:url";
import { Agent, request as httpsRequest } from "node:https";
import { rootCertificates } from "node:tls";
import { TLS_PORT } from "../testcontainers-tls/server.mjs";

const CONTAINER_PORT = TLS_PORT;
const STARTUP_TIMEOUT_MS = 20_000;
const REQUEST_TIMEOUT_MS = 5_000;
const MAX_RESPONSE_BYTES = 1024;
const JOB_IMAGE =
  "ghcr.io/tailrocks/velnor-job@sha256:b36946574402eb0c79fb6d7a9f7a77195d2ef943c92bc6b0fbc466ff3ea408f9";
const EXPECTED_CERTIFICATE_FILES = ["ca.pem", "server-key.pem", "server.pem"];
const UNTRUSTED_CERTIFICATE_CODES = new Set([
  "SELF_SIGNED_CERT_IN_CHAIN",
  "UNABLE_TO_GET_ISSUER_CERT",
  "UNABLE_TO_GET_ISSUER_CERT_LOCALLY",
  "UNABLE_TO_VERIFY_LEAF_SIGNATURE",
]);

const scriptDirectory = dirname(fileURLToPath(import.meta.url));
const serverSource = join(scriptDirectory, "../testcontainers-tls/server.mjs");

function assertDockerSdkEnvironment() {
  if (process.env.DOCKER_HOST !== "tcp://localhost:2375") {
    throw Object.assign(new Error("DOCKER_HOST must be exactly tcp://localhost:2375"), {
      code: "VELNOR_TLS_INVALID_DOCKER_HOST",
    });
  }
  if (process.env.TESTCONTAINERS_HOST_OVERRIDE !== undefined) {
    throw Object.assign(new Error("TESTCONTAINERS_HOST_OVERRIDE must be absent"), {
      code: "VELNOR_TLS_DOCKER_HOST_OVERRIDE_PRESENT",
    });
  }
  if (process.env.DOCKER_CONTEXT !== undefined) {
    throw Object.assign(new Error("DOCKER_CONTEXT must be absent"), {
      code: "VELNOR_TLS_DOCKER_CONTEXT_PRESENT",
    });
  }
}

export async function loadCertificateBundle(directory) {
  const requestedDirectory = resolve(directory);
  const requestedStats = await lstat(requestedDirectory);
  if (!requestedStats.isDirectory() || requestedStats.isSymbolicLink()) {
    throw new Error("certificate output path must be a real directory");
  }
  if ((requestedStats.mode & 0o077) !== 0) {
    throw new Error("certificate output directory must be private");
  }

  const certificateDirectory = await realpath(requestedDirectory);
  const names = (await readdir(certificateDirectory)).sort();
  if (names.length !== EXPECTED_CERTIFICATE_FILES.length ||
      names.some((name, index) => name !== EXPECTED_CERTIFICATE_FILES[index])) {
    throw new Error("certificate output directory must contain exactly the three generated PEM files");
  }

  const files = {};
  for (const name of EXPECTED_CERTIFICATE_FILES) {
    const path = join(certificateDirectory, name);
    const stats = await lstat(path);
    if (!stats.isFile() || stats.isSymbolicLink() || stats.size === 0) {
      throw new Error("generated certificate files must be nonempty regular files");
    }
    if (name === "server-key.pem" && (stats.mode & 0o777) !== 0o600) {
      throw new Error("generated server key must have mode 0600");
    }
    files[name] = path;
  }

  const ca = await readFile(files["ca.pem"]);
  if (ca.length === 0) {
    throw new Error("generated CA certificate is empty");
  }

  return { ca, directory: certificateDirectory, files };
}

function certificateAgent(ca) {
  // A fresh non-global Agent has no proxyEnv, so ambient proxy variables cannot
  // redirect the localhost TLS requests.
  return new Agent(ca === undefined ? { rejectUnauthorized: true } : { ca, rejectUnauthorized: true });
}

function requestText({ ca, host, path, port, servername }) {
  const url = new URL(`https://${host}:${port}${path}`);
  const agent = certificateAgent(ca);
  const options = { agent, method: "GET" };
  if (servername !== undefined) {
    // This changes only TLS SNI and the default hostname check. The URL host
    // remains localhost, and Node still performs its normal DNS lookup.
    options.servername = servername;
  }

  return new Promise((resolve, reject) => {
    const request = httpsRequest(url, options, (response) => {
      const chunks = [];
      let responseBytes = 0;
      response.on("data", (chunk) => {
        responseBytes += chunk.length;
        if (responseBytes > MAX_RESPONSE_BYTES) {
          request.destroy(Object.assign(new Error("fixture response exceeded its bound"), {
            code: "VELNOR_TLS_RESPONSE_TOO_LARGE",
          }));
          return;
        }
        chunks.push(chunk);
      });
      response.on("error", reject);
      response.on("end", () => {
        resolve({
          body: Buffer.concat(chunks).toString("utf8"),
          statusCode: response.statusCode,
        });
      });
    });
    const timeout = setTimeout(() => {
      request.destroy(Object.assign(new Error("fixture request exceeded its bound"), {
        code: "VELNOR_TLS_REQUEST_TIMEOUT",
      }));
    }, REQUEST_TIMEOUT_MS);

    request.on("close", () => clearTimeout(timeout));
    request.on("error", reject);
    request.end();
  }).finally(() => agent.destroy());
}

async function expectCertificateRejection(label, requestOptions, allowedCodes) {
  let failure;
  try {
    await requestText(requestOptions);
  } catch (error) {
    failure = error;
  }

  if (failure === undefined) {
    throw new Error(`${label} unexpectedly received an HTTPS response`);
  }
  const code = typeof failure.code === "string" ? failure.code : "unknown";
  if (!allowedCodes.has(code)) {
    throw new Error(`${label} failed for a non-certificate reason (${code})`);
  }
}

export async function runTlsChecks({ ca, host, nonce, port }) {
  if (host !== "localhost") {
    throw new Error("Testcontainers must report localhost for the mapped service port");
  }
  if (!Number.isInteger(port) || port < 1 || port > 65_535) {
    throw new Error("Testcontainers returned an invalid mapped service port");
  }
  if (typeof nonce !== "string" || !/^[0-9a-f]{64}$/.test(nonce)) {
    throw new Error("fixture nonce must be 32 random bytes encoded as lowercase hex");
  }

  const validIdentity = { ca, host, path: "/healthz", port };
  const health = await requestText(validIdentity);
  if (health.statusCode !== 200 || health.body !== "ok") {
    throw new Error("localhost HTTPS health check returned an unexpected response");
  }

  const nonceResponse = await requestText({ ...validIdentity, path: "/nonce" });
  if (nonceResponse.statusCode !== 200 || nonceResponse.body !== nonce) {
    throw new Error("localhost HTTPS nonce response did not match the generated nonce");
  }

  await expectCertificateRejection(
    "untrusted CA",
    { ca: rootCertificates, host, path: "/nonce", port },
    UNTRUSTED_CERTIFICATE_CODES,
  );
  await expectCertificateRejection(
    "wrong DNS identity",
    { ca, host, path: "/nonce", port, servername: "wrong.invalid" },
    new Set(["ERR_TLS_CERT_ALTNAME_INVALID"]),
  );
  await expectCertificateRejection(
    "IP identity",
    { ca, host: "127.0.0.1", path: "/nonce", port },
    new Set(["ERR_TLS_CERT_ALTNAME_INVALID"]),
  );

  const nonceAfterRejections = await requestText({ ...validIdentity, path: "/nonce" });
  if (nonceAfterRejections.statusCode !== 200 || nonceAfterRejections.body !== nonce) {
    throw new Error("localhost HTTPS endpoint changed after the certificate-rejection checks");
  }
}

async function runTestcontainersQualification(certificateDirectory) {
  assertDockerSdkEnvironment();
  const certificates = await loadCertificateBundle(certificateDirectory);
  const serverStats = await lstat(serverSource);
  if (!serverStats.isFile() || serverStats.isSymbolicLink()) {
    throw new Error("shared TLS server source must be a regular file");
  }

  const nonce = randomBytes(32).toString("hex");
  const { GenericContainer, Wait } = await import("testcontainers");
  let container;
  let operationFailure;
  let cleanupFailure;

  try {
    container = await new GenericContainer(JOB_IMAGE)
      .withCopyFilesToContainer([
        {
          mode: 0o644,
          source: serverSource,
          target: "/tmp/velnor-tls-server.mjs",
        },
        {
          mode: 0o644,
          source: certificates.files["server.pem"],
          target: "/tmp/velnor-tls-server.pem",
        },
        {
          mode: 0o600,
          source: certificates.files["server-key.pem"],
          target: "/tmp/velnor-tls-server-key.pem",
        },
      ])
      .withEnvironment({ VELNOR_TLS_NONCE: nonce })
      .withCommand(["node", "/tmp/velnor-tls-server.mjs"])
      .withExposedPorts(CONTAINER_PORT)
      .withWaitStrategy(Wait.forListeningPorts())
      .withStartupTimeout(STARTUP_TIMEOUT_MS)
      .start();

    const host = container.getHost();
    const port = container.getMappedPort(CONTAINER_PORT);
    await runTlsChecks({ ca: certificates.ca, host, nonce, port });
  } catch (error) {
    operationFailure = error;
  } finally {
    if (container !== undefined) {
      try {
        await container.stop({ timeout: 0 });
      } catch (error) {
        cleanupFailure = error;
      }
    }
  }

  if (operationFailure !== undefined && cleanupFailure !== undefined) {
    throw new AggregateError(
      [operationFailure, cleanupFailure],
      "TLS qualification failed and the owned service container did not stop cleanly",
    );
  }
  if (cleanupFailure !== undefined) {
    throw new Error(`owned service container cleanup failed (${errorCode(cleanupFailure)})`);
  }
  if (operationFailure !== undefined) {
    throw operationFailure;
  }

  console.log("testcontainers-tls-ok");
  console.log("testcontainers-tls-rejected-untrusted-ca");
  console.log("testcontainers-tls-rejected-wrong-dns");
  console.log("testcontainers-tls-rejected-ip");
}

function errorCode(error) {
  if (error instanceof AggregateError) {
    return error.errors.map(errorCode).join(",");
  }
  return typeof error?.code === "string" ? error.code : error?.name ?? "unknown";
}

function invokedDirectly() {
  return process.argv[1] !== undefined &&
    pathToFileURL(resolve(process.argv[1])).href === import.meta.url;
}

if (invokedDirectly()) {
  const args = process.argv.slice(2);
  if (args.length !== 1) {
    console.error("usage: node qualification/testcontainers/tls.mjs <private-certificate-directory>");
    process.exitCode = 2;
  } else {
    runTestcontainersQualification(args[0]).catch((error) => {
      console.error(`testcontainers-tls-failed (${errorCode(error)})`);
      process.exitCode = 1;
    });
  }
}

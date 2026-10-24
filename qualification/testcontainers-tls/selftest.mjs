import { randomBytes } from "node:crypto";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { loadCertificateBundle, runTlsChecks } from "../testcontainers/tls.mjs";
import { createTlsServer } from "./server.mjs";

async function listen(server, host, port) {
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(port, host, resolve);
  });
}

async function close(server) {
  if (!server.listening) {
    return;
  }
  await new Promise((resolve, reject) => {
    server.close((error) => error ? reject(error) : resolve());
  });
}

function errorCode(error) {
  return typeof error?.code === "string" ? error.code : error?.name ?? "unknown";
}

async function runSelfTest(certificateDirectory) {
  const certificates = await loadCertificateBundle(certificateDirectory);
  const nonce = randomBytes(32).toString("hex");
  const certificate = await readFile(certificates.files["server.pem"]);
  const privateKey = await readFile(certificates.files["server-key.pem"]);
  const options = { certificate, nonce, privateKey };
  const ipv6Server = createTlsServer(options);
  const ipv4Server = createTlsServer(options);
  let failure;

  try {
    await listen(ipv6Server, "::1", 0);
    const address = ipv6Server.address();
    if (address === null || typeof address === "string") {
      throw new Error("self-test server did not receive a TCP port");
    }
    await listen(ipv4Server, "127.0.0.1", address.port);
    await runTlsChecks({ ca: certificates.ca, host: "localhost", nonce, port: address.port });
  } catch (error) {
    failure = error;
  }

  const cleanupErrors = [];
  for (const server of [ipv4Server, ipv6Server]) {
    try {
      await close(server);
    } catch (error) {
      cleanupErrors.push(error);
    }
  }

  if (failure !== undefined && cleanupErrors.length > 0) {
    throw new AggregateError([failure, ...cleanupErrors], "TLS self-test and cleanup failed");
  }
  if (cleanupErrors.length > 0) {
    throw new AggregateError(cleanupErrors, "TLS self-test server cleanup failed");
  }
  if (failure !== undefined) {
    throw failure;
  }

  console.log("testcontainers-tls-selftest-ok");
  console.log("testcontainers-tls-rejected-untrusted-ca");
  console.log("testcontainers-tls-rejected-wrong-dns");
  console.log("testcontainers-tls-rejected-ip");
}

const direct = process.argv[1] !== undefined &&
  pathToFileURL(resolve(process.argv[1])).href === import.meta.url;
if (direct) {
  const args = process.argv.slice(2);
  if (args.length !== 1) {
    console.error("usage: node qualification/testcontainers-tls/selftest.mjs <private-certificate-directory>");
    process.exitCode = 2;
  } else {
    runSelfTest(args[0]).catch((error) => {
      console.error(`testcontainers-tls-selftest-failed (${errorCode(error)})`);
      process.exitCode = 1;
    });
  }
}

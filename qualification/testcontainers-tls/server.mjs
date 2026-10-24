import { createServer } from "node:https";
import { lstatSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

export const TLS_PORT = 8443;
const CERTIFICATE_PATH = "/tmp/velnor-tls-server.pem";
const PRIVATE_KEY_PATH = "/tmp/velnor-tls-server-key.pem";

export function createTlsServer({ certificate, nonce, privateKey }) {
  if (typeof nonce !== "string" || !/^[0-9a-f]{64}$/.test(nonce)) {
    throw new Error("fixture nonce must be 32 random bytes encoded as lowercase hex");
  }

  return createServer({ cert: certificate, key: privateKey }, (request, response) => {
    if (request.method !== "GET") {
      response.writeHead(405, { "content-length": "0" });
      response.end();
      return;
    }

    const body = request.url === "/healthz"
      ? "ok"
      : request.url === "/nonce"
        ? nonce
        : undefined;
    if (body === undefined) {
      response.writeHead(404, {
        "content-length": "9",
        "content-type": "text/plain; charset=utf-8",
      });
      response.end("not found");
      return;
    }

    response.writeHead(200, {
      "cache-control": "no-store",
      "content-length": String(Buffer.byteLength(body)),
      "content-type": "text/plain; charset=utf-8",
    });
    response.end(body);
  });
}

function invokedDirectly() {
  return process.argv[1] !== undefined &&
    pathToFileURL(resolve(process.argv[1])).href === import.meta.url;
}

if (invokedDirectly()) {
  const nonce = process.env.VELNOR_TLS_NONCE;
  let server;
  try {
    const privateKeyStats = lstatSync(PRIVATE_KEY_PATH);
    if (!privateKeyStats.isFile() || privateKeyStats.isSymbolicLink() ||
        (privateKeyStats.mode & 0o777) !== 0o600) {
      throw new Error("copied server key must be a regular file with mode 0600");
    }
    server = createTlsServer({
      certificate: readFileSync(CERTIFICATE_PATH),
      nonce,
      privateKey: readFileSync(PRIVATE_KEY_PATH),
    });
  } catch (error) {
    const code = typeof error?.code === "string" ? error.code : error?.name ?? "unknown";
    console.error(`velnor-tls-server-failed (${code})`);
    process.exitCode = 1;
  }

  if (server !== undefined) {
    server.on("error", (error) => {
      const code = typeof error?.code === "string" ? error.code : error?.name ?? "unknown";
      console.error(`velnor-tls-server-failed (${code})`);
      process.exitCode = 1;
    });
    server.listen(TLS_PORT, "0.0.0.0", () => {
      console.log("velnor-tls-listening");
    });
    for (const signal of ["SIGINT", "SIGTERM"]) {
      process.once(signal, () => {
        server.close(() => process.exit(0));
      });
    }
  }
}

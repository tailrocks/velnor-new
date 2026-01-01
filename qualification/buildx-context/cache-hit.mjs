import { readFileSync } from "node:fs";

function cachedCopy(log) {
  const copyVertices = [...log.matchAll(/^#([0-9]+(?:\.[0-9]+)*)\s+.*\bCOPY payload\.txt \/payload\.txt\s*$/gm)];
  if (copyVertices.length !== 1) return false;
  const id = copyVertices[0][1].replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return new RegExp(`^#${id}\\s+CACHED\\s*$`, "m").test(log);
}

function selfTest() {
  const positive = "#4 [1/2] FROM scratch\n#4 CACHED\n#5 [2/2] COPY payload.txt /payload.txt\n#5 CACHED\n";
  const unrelatedCached = "#5 [2/2] COPY payload.txt /payload.txt\n#8 CACHED\n";
  const noncachedCopy = "#5 [2/2] COPY payload.txt /payload.txt\n#5 DONE 0.0s\n#8 CACHED\n";
  if (!cachedCopy(positive) || cachedCopy(unrelatedCached) || cachedCopy(noncachedCopy)) {
    throw new Error("BuildKit COPY vertex cache parser self-test failed");
  }
  console.log("buildx-copy-cache-parser-ok");
}

if (process.argv[2] === "--self-test") selfTest();
else if (process.argv[2]) {
  if (!cachedCopy(readFileSync(process.argv[2], "utf8"))) {
    throw new Error("the exact COPY payload vertex was not reported CACHED");
  }
  console.log("buildx-copy-cache-hit-ok");
} else {
  throw new Error("usage: cache-hit.mjs <build log> | --self-test");
}

import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import vm from "node:vm";

import {
    FinalizeCacheEntryUploadRequest,
    FinalizeCacheEntryUploadResponse
} from "../node_modules/@actions/cache/lib/generated/results/api/v1/cache.js";

const read = relative =>
    fs.readFileSync(new URL(relative, import.meta.url), "utf8");
const hash = source => crypto.createHash("sha256").update(source).digest("hex");

function patchedSource(patch) {
    const target = new URL(`../${patch.path}`, import.meta.url);
    let source = fs.readFileSync(target, "utf8");
    for (const replacement of patch.replacements) {
        if (source.includes(replacement.to)) continue;
        assert.equal(hash(source), patch.upstreamSha256);
        assert.equal(source.split(replacement.from).length, 2);
        source = source.replace(replacement.from, replacement.to);
    }
    return source;
}

const cachePatch = JSON.parse(read("service-receipt-patch.json"));
const cacheSource = patchedSource(cachePatch);
if (!/Number\.isSafeInteger\(cacheId\)/.test(cacheSource)) {
    throw new Error("cache source lacks safe publication-ID validation");
}
if (/cacheId = parseInt\(finalizeResponse\.entryId\)/.test(cacheSource)) {
    throw new Error("cache source still uses parseInt for publication IDs");
}

for (const bundle of ["../dist/save-only/index.js", "../dist/save/index.js"]) {
    const source = read(bundle);
    if (/cacheId = parseInt\(finalizeResponse\.entryId\)/.test(source)) {
        throw new Error(`${bundle} still uses parseInt for publication IDs`);
    }
    if (!/Invalid finalized cache entry ID/.test(source)) {
        throw new Error(`${bundle} lacks safe publication-ID validation`);
    }
}

const rpcPatch = JSON.parse(read("service-rpc-patch.json"));
const rpcSource = patchedSource(rpcPatch);
const classStart = rpcSource.indexOf("export class CacheServiceClientJSON");
const classEnd = rpcSource.indexOf("export class CacheServiceClientProtobuf");
assert.ok(classStart >= 0 && classEnd > classStart);
const classSource = rpcSource
    .slice(classStart, classEnd)
    .replace("export class", "class");
const clientContext = {
    FinalizeCacheEntryUploadRequest,
    FinalizeCacheEntryUploadResponse
};
vm.createContext(clientContext);
vm.runInContext(
    `${classSource}\nthis.Client = CacheServiceClientJSON;`,
    clientContext
);

const receiptPatch = cachePatch.replacements[0].to;
const receiptContext = {};
vm.createContext(receiptContext);
vm.runInContext(
    `this.decode = function (entryId) {
    const finalizeResponse = { entryId };
    let cacheId = -1;
    ${receiptPatch}
    return cacheId;
  }`,
    receiptContext
);

async function finalize(responseBody) {
    const seen = {};
    const client = new clientContext.Client({
        request(_service, method, contentType, data) {
            Object.assign(seen, { method, contentType, data });
            return Promise.resolve(responseBody);
        }
    });
    const response = await client.FinalizeCacheEntryUpload({
        key: "receipt-key",
        sizeBytes: "1",
        version: "receipt-version"
    });
    return { response, seen };
}

const valid = await finalize({ ok: true, entry_id: "42" });
assert.equal(valid.seen.method, "FinalizeCacheEntryUpload");
assert.equal(valid.seen.contentType, "application/json");
assert.equal(valid.response.entryId, "42");
assert.equal(receiptContext.decode(valid.response.entryId), 42);

const camelValid = await finalize({ ok: true, entryId: "43" });
assert.equal(camelValid.response.entryId, "43");
assert.equal(receiptContext.decode(camelValid.response.entryId), 43);

const explicitZero = await finalize({ ok: true, entry_id: "0" });
assert.equal(explicitZero.response.entryId, "0");
assert.equal(receiptContext.decode(explicitZero.response.entryId), 0);

const maxSafe = await finalize({ ok: true, entry_id: "9007199254740991" });
assert.equal(maxSafe.response.entryId, "9007199254740991");
assert.equal(receiptContext.decode(maxSafe.response.entryId), 9007199254740991);

const failed = await finalize({ ok: false, message: "finalization conflict" });
assert.equal(failed.response.ok, false);

for (const responseBody of [
    { ok: true },
    { ok: true, entry_id: null },
    { ok: true, entry_id: "0x2a" },
    { ok: true, entry_id: "+42" },
    { ok: true, entry_id: "-1" },
    { ok: true, entry_id: " 42" },
    { ok: true, entry_id: "042" },
    { ok: true, entry_id: 42 },
    { ok: true, entry_id: { toString: () => "42" } },
    { ok: true, entry_id: "9007199254740992" },
    { ok: true, entry_id: "42", entryId: "42" }
]) {
    await assert.rejects(() => finalize(responseBody));
}

console.log(
    JSON.stringify({
        cacheSourceGuard: true,
        bundleGuards: 2,
        rpcSourceGuard: true,
        validIds: [42, 43],
        explicitZero: 0,
        maxSafeId: 9007199254740991,
        rejectedResponses: 11
    })
);

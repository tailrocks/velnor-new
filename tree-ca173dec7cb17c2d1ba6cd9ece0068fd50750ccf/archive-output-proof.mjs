import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { createTar, extractTar } from '../node_modules/@actions/cache/lib/internal/tar.js';

const root = fs.mkdtempSync(path.join(os.tmpdir(), 'velnor-output-proof-'));
const workspace = path.join(root, 'workspace with spaces');
const payload = path.join(workspace, 'payload');
fs.mkdirSync(payload, { recursive: true, mode: 0o700 });
const filenames = ['cache.tzst', 'cache.tgz', 'ordinary\nfile'];
for (const name of filenames) fs.writeFileSync(path.join(payload, name), `public fixture ${name}\n`, { mode: 0o755 });
process.env.GITHUB_WORKSPACE = workspace;
process.env.RUNNER_TEMP = root;
const results = [];
for (const [index, method] of ['gzip', 'zstd-without-long'].entries()) {
  const output = path.join(root, `output-${index}`);
  fs.mkdirSync(output, { mode: 0o700 });
  await createTar(output, [payload], method);
  const archive = path.join(output, method === 'gzip' ? 'cache.tgz' : 'cache.tzst');
  const quarantinePath = path.join(root, 'velnor/cache-staging', String(index + 1).repeat(64));
  await extractTar(archive, method, { roots: [payload], quarantinePath });
  for (const name of filenames) {
    const staged = path.join(quarantinePath, 'roots/0', name);
    assert.deepEqual(fs.readFileSync(staged), fs.readFileSync(path.join(payload, name)));
    assert.equal(fs.statSync(staged).mode & 0o777, 0o755);
  }
  const nested = path.join(payload, `nested-output-${index}`);
  fs.mkdirSync(nested, { mode: 0o700 });
  const alias = path.join(root, `aliased-output-${index}`);
  fs.symlinkSync(nested, alias);
  for (const folder of [nested, alias]) {
    await assert.rejects(createTar(folder, [payload], method),
      error => error.code === 'ARCHIVE_OUTPUT_OVERLAP');
    assert.deepEqual(fs.readdirSync(nested), []);
  }
  fs.rmdirSync(nested);
  results.push({ method, completePayload: true, names: filenames,
    nestedOutputRejectedBeforeWrites: true, actualAliasedOutputRejected: true,
    archiveSha256: createHash('sha256').update(fs.readFileSync(archive)).digest('hex') });
}
console.log(JSON.stringify({ node: process.versions.node, platform: process.platform,
  root, results, qualified: false, scope: 'fresh quiescent job namespace' }, null, 2));

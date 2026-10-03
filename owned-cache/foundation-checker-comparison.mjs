// Pure verification equivalence fixture. No profile issuance or native GH run.
import fs from 'node:fs';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { inventoryFoundation, canonicalInventory } from './foundation-python-inventory.mjs';

if (process.platform !== 'linux' || process.getuid() !== 0)
  throw new Error('checker comparison requires isolated Linux root fixture');
const base = fs.mkdtempSync('/foundation-checker-fixture-');
const sdk = `${base}/sdk`;
const control = `${base}/control`;
const digest = value => crypto.createHash('sha256').update(value).digest('hex');
try {
  fs.chmodSync(base, 0o755);
  fs.mkdirSync(sdk, { mode: 0o755 });
  fs.mkdirSync(control, { mode: 0o700 });
  fs.writeFileSync(`${sdk}/owned.bin`, 'original', { mode: 0o644 });
  fs.writeFileSync(`${sdk}/\uf000`, 'BMP', { mode: 0o644 });
  fs.writeFileSync(`${sdk}/\u{1f600}`, 'non-BMP', { mode: 0o644 });
  fs.symlinkSync('owned.bin', `${sdk}/alias`);
  const roots = [sdk, `${sdk}/future/config`, '/etc/ssl/certs/ca-certificates.crt'];
  const inventory = inventoryFoundation(roots);
  const record = { roots, inventory, executable: `${sdk}/owned.bin`, sources: [] };
  const raw = Buffer.from(JSON.stringify(record));
  fs.writeFileSync(`${control}/foundation-observation.json`, raw, { mode: 0o600 });
  const namespace = [];
  let directory = control;
  for (;;) {
    const info = fs.lstatSync(directory, { bigint: true });
    namespace.push({ path: directory, device: String(info.dev), inode: String(info.ino),
      mode: Number(info.mode & 0o7777n), uid: Number(info.uid), gid: Number(info.gid) });
    if (directory === '/') break;
    directory = directory.slice(0, directory.lastIndexOf('/')) || '/';
  }
  const seed = { record, control_root: control, payload_roots: [sdk],
    ca_file: '/etc/ssl/certs/ca-certificates.crt', namespace,
    observation_sha256: digest(raw), source_root: import.meta.dirname };
  const checker = fs.readFileSync(`${import.meta.dirname}/foundation-fresh-gh-checker.py`, 'utf8');
  const literal = Buffer.from(JSON.stringify(seed)).toString('hex');
  const python = `import json\n${checker}\n` +
    `seed=json.loads(bytes.fromhex('${literal}'))\n` +
    `try:\n    require_current(seed)\nexcept Exception:\n    print('false')\nelse:\n    print('true')\n`;
  let probes = 0;
  function compare(expected) {
    let node = false;
    try { node = canonicalInventory(inventoryFoundation(roots)) === canonicalInventory(inventory); }
    catch { node = false; }
    const result = spawnSync('/usr/bin/python3', ['-I', '-S', '-B', '-'], {
      input: python, cwd: '/', env: { LANG: 'C', LC_ALL: 'C' },
      timeout: 30000, maxBuffer: 1024 * 1024, encoding: 'utf8'
    });
    assert.equal(result.status, 0);
    const translated = result.stdout.trim() === 'true';
    assert.equal(node, expected);
    assert.equal(translated, node);
    probes++;
  }
  compare(true);
  fs.writeFileSync(`${sdk}/owned.bin`, 'modified'); compare(false);
  fs.writeFileSync(`${sdk}/owned.bin`, 'original'); compare(true);
  fs.writeFileSync(`${sdk}/injected.pyc`, 'new'); compare(false);
  fs.unlinkSync(`${sdk}/injected.pyc`);
  fs.mkdirSync(`${sdk}/future`, { mode: 0o755 }); compare(false);
  fs.rmdirSync(`${sdk}/future`);
  fs.unlinkSync(`${sdk}/alias`); fs.symlinkSync('\uf000', `${sdk}/alias`); compare(false);
  fs.unlinkSync(`${sdk}/alias`); fs.symlinkSync('owned.bin', `${sdk}/alias`);
  fs.chmodSync(`${sdk}/owned.bin`, 0o666); compare(false);
  fs.chmodSync(`${sdk}/owned.bin`, 0o644); compare(true);
  process.stdout.write(`${probes} JS/Python original-inventory equivalence probes passed; fixture only.\n`);
} finally { fs.rmSync(base, { recursive: true, force: true }); }

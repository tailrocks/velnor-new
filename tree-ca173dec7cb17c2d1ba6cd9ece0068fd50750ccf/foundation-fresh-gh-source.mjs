// Fixed descriptive SDK source assets for the private initial Node issuer.
// This module accepts no caller seed and issues no profile or executable capsule.
// Environment, downloaded code and cache payloads cannot supply source assets.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { SDK_GH_DISTRIBUTIONS } from './fresh-gh-sdk/sdk-gh-distributions.mjs';

const MAX_SOURCE = 64 * 1024 * 1024;
const HERE = import.meta.dirname;
const MODULES = Object.freeze([
  ['cache_receipt_common', 'fresh-gh-sdk/cache_receipt_common.py'],
  ['foundation_fresh_gh_checker', 'foundation-fresh-gh-checker.py'],
  ['foundation_fresh_gh_profile', 'foundation-fresh-gh-profile.py'],
  ['cache_receipt_api', 'fresh-gh-sdk/cache_receipt_api.py'],
  ['cache_receipt_gh', 'fresh-gh-sdk/cache_receipt_gh.py'],
  ['receipt_fresh_gh_download', 'fresh-gh-sdk/receipt_fresh_gh_download.py'],
  ['receipt_fresh_gh_archive', 'fresh-gh-sdk/receipt_fresh_gh_archive.py'],
  ['receipt_fresh_gh', 'fresh-gh-sdk/receipt_fresh_gh.py'],
  ['foundation_fresh_gh_positive', 'foundation-fresh-gh-positive.py']
]);
const EVIDENCE = Object.freeze(['trusted_root.json', 'trusted_root.jsonl', 'qualification.json',
  'bootstrap_root.json', '15.root.json', '798.timestamp.json', '165.snapshot.json',
  '14.targets.json', 'public_release_bundle.json', 'public_release_verification.json']);
const ASSETS = Object.freeze([...new Set([
  'foundation-fresh-gh-source.mjs', 'export-fresh-gh-sdk.py',
  ...MODULES.map(([, filename]) => filename),
  'fresh-gh-sdk/catalog_qualification_gh.rs',
  'fresh-gh-sdk/catalog_qualification_types.rs',
  'fresh-gh-sdk/catalog_qualification_identity.rs',
  'fresh-gh-sdk/cache_receipt_trust_root.rs',
  'fresh-gh-sdk/sdk-gh-distributions.mjs', 'fresh-gh-sdk/research-extraction.json',
  ...EVIDENCE.map(filename => `fresh-gh-sdk/root/${filename}`)
])].sort());
const digest = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
const decode = bytes => new TextDecoder('utf-8', { fatal: true }).decode(bytes);

function sourceBytes(filename) {
  const descriptor = fs.openSync(path.join(HERE, filename),
    fs.constants.O_RDONLY | fs.constants.O_NOFOLLOW | fs.constants.O_NONBLOCK);
  try {
    const before = fs.fstatSync(descriptor);
    if (!before.isFile() || before.nlink !== 1 || before.size > MAX_SOURCE)
      throw new Error('Fresh GH owned source shape');
    const bytes = fs.readFileSync(descriptor);
    const after = fs.fstatSync(descriptor);
    if (bytes.length !== before.size || before.ino !== after.ino ||
        before.size !== after.size || before.ctimeMs !== after.ctimeMs ||
        before.mtimeMs !== after.mtimeMs) throw new Error('Fresh GH source changed');
    return bytes;
  } finally { fs.closeSync(descriptor); }
}

const HELD = new Map(ASSETS.map(filename => [filename, sourceBytes(filename)]));
const DISTRIBUTIONS = JSON.parse(JSON.stringify(SDK_GH_DISTRIBUTIONS));
Object.values(DISTRIBUTIONS).forEach(Object.freeze);
Object.freeze(DISTRIBUTIONS);

export function sourceAssetsManifest() {
  return ASSETS.map(filename => ({ path: filename, bytes: HELD.get(filename).length,
    sha256: digest(HELD.get(filename)) }));
}

export function requireFreshGhSourceCurrent() {
  for (const filename of ASSETS)
    if (!sourceBytes(filename).equals(HELD.get(filename)))
      throw new Error('Fresh GH owned source changed');
}

// Fixed descriptive bytes only. Profile-bearing source compilation belongs to
// the private Foundation issuer class; this module accepts no caller seed.
export function freshGhSourceAssets() {
  return Object.freeze({
    sources: Object.freeze(Object.fromEntries(MODULES.map(([name, filename]) =>
      [name, decode(HELD.get(filename))]))),
    names: Object.freeze(MODULES.map(([name]) => name)),
    rootHex: HELD.get('fresh-gh-sdk/root/trusted_root.jsonl').toString('hex'),
    bundleHex: HELD.get('fresh-gh-sdk/root/public_release_bundle.json').toString('hex'),
    distributions: DISTRIBUTIONS,
    manifest: sourceAssetsManifest()
  });
}

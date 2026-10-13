import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { spawn, spawnSync } from 'node:child_process';
import { collectFoundationPython } from './foundation-python-collector.mjs';
import { inventoryFoundation, canonicalInventory } from './foundation-python-inventory.mjs';
import { FOUNDATION_CA_FILE, FOUNDATION_ARCHIVE_ALIASES }
  from './foundation-native-loader.mjs';
import { freshGhSourceAssets, sourceAssetsManifest, requireFreshGhSourceCurrent }
  from './foundation-fresh-gh-source.mjs';

const FIXED_TEMP = '/home/runner/work/_temp';
const CONTROL_PARENT = `${FIXED_TEMP}/velnor-foundation-control`;
const ENVIRONMENT = Object.freeze({ LANG: 'C', LC_ALL: 'C' });
const CANONICAL_SHA = '3bba7f21fc728bea94c1d8202611537a1f8791585313e126a7a00b0249823fb1';
const SOURCE_FILES = Object.freeze([
  'foundation-native-bootstrap.mjs', 'foundation-native-loader.mjs',
  'foundation-native-openssl.mjs',
  'foundation-native-positive.py', 'foundation-native-entrypoint.mjs',
  'foundation-python-collector.mjs', 'foundation-python-inventory.mjs',
  'metadata_container.py'
]);
const ISSUER_TOKEN = Symbol('owned initial fresh SDK issuer');
const digest = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
const OWNED_SOURCE_BYTES = new Map(SOURCE_FILES.map(filename => [filename,
  fs.readFileSync(path.join(import.meta.dirname, filename))]));

function namespaceIdentity(directory) {
  const identity = [];
  let current = directory;
  for (;;) {
    const stat = fs.lstatSync(current, { bigint: true });
    if (!stat.isDirectory() || stat.isSymbolicLink() ||
        ![0n, BigInt(process.getuid())].includes(stat.uid) || (stat.mode & 0o022n) !== 0n)
      throw new Error('Foundation control namespace');
    identity.push({ path: current, device: String(stat.dev), inode: String(stat.ino),
      mode: Number(stat.mode & 0o7777n), uid: Number(stat.uid), gid: Number(stat.gid) });
    if (current === '/') break;
    current = path.dirname(current);
  }
  return identity;
}

function sourceManifest() {
  const main = SOURCE_FILES.map(filename => {
    const bytes = fs.readFileSync(path.join(import.meta.dirname, filename));
    return { path: filename, bytes: bytes.length, sha256: digest(bytes) };
  });
  return [...main, ...sourceAssetsManifest()].sort((left, right) =>
    left.path < right.path ? -1 : left.path > right.path ? 1 : 0);
}

function invoke(executable, argv, input) {
  const result = spawnSync(executable, argv, { env: ENVIRONMENT, cwd: '/',
    input, timeout: 90000, maxBuffer: 4 * 1024 * 1024, encoding: 'utf8', shell: false });
  if (result.error || result.status !== 0) throw new Error('Foundation native positive failed');
  return result.stdout;
}

function sourceLiteral(value) {
  const data = Buffer.from(JSON.stringify(value), 'utf8');
  if (data.length > 32 * 1024 * 1024) throw new Error('Fresh GH literal bound');
  return `__import__('json').loads(bytes.fromhex('${data.toString('hex')}'))`;
}

function sourceReplace(source, marker, replacement) {
  if (source.split(marker).length !== 2) throw new Error('Fresh GH source hook count');
  return source.replace(marker, replacement);
}

class FreshFoundationObservation {
  #record; #identity; #manifestDigest;
  constructor(token, record, controlRoot) {
    if (token !== ISSUER_TOKEN) throw new Error('Foundation private issuer');
    this.#record = record;
    this.control_root = controlRoot;
    this.payload_roots = Object.freeze([process.env.GITHUB_WORKSPACE, `${FIXED_TEMP}/velnor`]);
    this.ca_file = FOUNDATION_CA_FILE;
    this.#identity = namespaceIdentity(controlRoot);
    const bytes = Buffer.from(JSON.stringify(record));
    this.#manifestDigest = digest(bytes);
    fs.writeFileSync(`${controlRoot}/foundation-observation.json`, bytes,
      { flag: 'wx', mode: 0o600 });
    Object.freeze(this);
  }
  require_current() {
    requireFreshGhSourceCurrent();
    if (canonicalInventory(sourceManifest()) !== canonicalInventory(this.#record.sources))
      throw new Error('Foundation source closure changed');
    if (canonicalInventory(namespaceIdentity(this.control_root)) !==
        canonicalInventory(this.#identity)) throw new Error('Foundation namespace changed');
    for (const payload of this.payload_roots) {
      if (typeof payload !== 'string' || !path.isAbsolute(payload) ||
          path.normalize(payload) !== payload) throw new Error('Foundation payload namespace');
      const real = fs.existsSync(payload) ? fs.realpathSync(payload) : payload;
      for (const compared of [payload, real]) {
        const relative = path.relative(this.control_root, compared);
        const reverse = path.relative(compared, this.control_root);
        const outside = value => value === '..' || value.startsWith(`..${path.sep}`) ||
          path.isAbsolute(value);
        if (!relative || !outside(relative) || !reverse || !outside(reverse))
          throw new Error('Foundation control overlaps payload');
      }
    }
    const manifest = `${this.control_root}/foundation-observation.json`;
    const stat = fs.lstatSync(manifest);
    if (!stat.isFile() || stat.uid !== process.getuid() || (stat.mode & 0o077) !== 0 ||
        digest(fs.readFileSync(manifest)) !== this.#manifestDigest)
      throw new Error('Foundation observation changed');
    if (canonicalInventory(inventoryFoundation(this.#record.roots)) !==
        canonicalInventory(this.#record.inventory)) throw new Error('Foundation closure changed');
  }
  qualification_positive() {
    this.require_current();
    const canonical = OWNED_SOURCE_BYTES.get('metadata_container.py');
    if (digest(canonical) !== CANONICAL_SHA) throw new Error('Foundation canonical source');
    const source = OWNED_SOURCE_BYTES.get('foundation-native-positive.py').toString('utf8');
    if (digest(source) !== this.#record.sources.find(item =>
      item.path === 'foundation-native-positive.py').sha256)
      throw new Error('Foundation positive source changed');
    const positive = JSON.parse(invoke(this.#record.executable,
      ['-I', '-S', '-B', '-c', source], canonical));
    const aliases = FOUNDATION_ARCHIVE_ALIASES.map(alias => {
      const version = invoke(alias, ['--version']);
      return { path: alias, resolved: fs.realpathSync(alias),
        version_stdout: version, version_sha256: digest(version) };
    });
    this.require_current();
    const receipt = { schema: 1, state: 'native-positive', authority: false,
      host: this.#record.host, canonical_sha256: CANONICAL_SHA,
      control_root: this.control_root, payload_roots: this.payload_roots,
      ca_file: this.ca_file, sdk_observation_sha256: this.#manifestDigest,
      sources: sourceManifest(), positive, aliases,
      run: { repository: process.env.GITHUB_REPOSITORY, run_id: process.env.GITHUB_RUN_ID,
        run_attempt: process.env.GITHUB_RUN_ATTEMPT, workflow_ref: process.env.GITHUB_WORKFLOW_REF,
        workflow_sha: process.env.GITHUB_WORKFLOW_SHA, action_ref: process.env.GITHUB_ACTION_REF,
        action_repository: process.env.GITHUB_ACTION_REPOSITORY }
    };
    const bytes = Buffer.from(JSON.stringify(receipt));
    const receiptPath = `${this.control_root}/native-positive.json`;
    fs.writeFileSync(receiptPath, bytes, { flag: 'wx', mode: 0o600 });
    return { receiptPath, receiptSha256: digest(bytes),
      observationPath: `${this.control_root}/foundation-observation.json`,
      observationSha256: this.#manifestDigest, controlRoot: this.control_root };
  }
  #assembleFreshGhQualification() {
    const seed = { record: this.#record,
      control_root: this.control_root, payload_roots: this.payload_roots,
      ca_file: this.ca_file, namespace: this.#identity,
      observation_sha256: this.#manifestDigest, source_root: import.meta.dirname };
    const assets = freshGhSourceAssets();
    const distribution = assets.distributions[this.#record.host.architecture === 'x64'
      ? 'amd64' : 'arm64'];
    const sources = { ...assets.sources };
    sources.foundation_fresh_gh_profile = sourceReplace(sources.foundation_fresh_gh_profile,
      '_PRIVATE_FOUNDATION_SEED = None', `_PRIVATE_FOUNDATION_SEED = ${sourceLiteral(seed)}`);
    sources.cache_receipt_gh = sourceReplace(sources.cache_receipt_gh,
      '_COMPILED_GH_DISTRIBUTION = None', `_COMPILED_GH_DISTRIBUTION = ${sourceLiteral(distribution)}`);
    let fresh = sourceReplace(sources.receipt_fresh_gh, '    return None\n',
      '    from foundation_fresh_gh_profile import current\n    return current()\n');
    fresh = sourceReplace(fresh, '_COMPILED_TRUSTED_ROOT = None',
      `_COMPILED_TRUSTED_ROOT = bytes.fromhex('${assets.rootHex}')`);
    sources.receipt_fresh_gh = sourceReplace(fresh, '_COMPILED_REPOSITORY = None',
      "_COMPILED_REPOSITORY = 'cli/cli'");
    sources.foundation_fresh_gh_positive = sourceReplace(sources.foundation_fresh_gh_positive,
      '_COMPILED_PUBLIC_BUNDLE = None', `_COMPILED_PUBLIC_BUNDLE = bytes.fromhex('${assets.bundleHex}')`);
    const script = `import sys, types\n_SOURCES = ${sourceLiteral(sources)}\n` +
      `_ORDER = ${sourceLiteral(assets.names)}\n` +
      `for _name in _ORDER:\n` +
      `    _module = types.ModuleType(_name)\n` +
      `    _module.__file__ = '<initial-owned-sdk:' + _name + '>'\n` +
      `    sys.modules[_name] = _module\n` +
      `for _name in _ORDER:\n` +
      `    exec(compile(_SOURCES[_name], '<initial-owned-sdk:' + _name + '>', 'exec'), sys.modules[_name].__dict__)\n`;
    const source = Buffer.from(script, 'utf8');
    if (source.length > 64 * 1024 * 1024) throw new Error('Fresh GH capsule bound');
    return { source, sourceSha256: digest(source), manifest: assets.manifest,
      scope: 'initial-owned-sdk-public-release-verifier-compatibility-only', authority: false };
  }
  async launchFreshGhQualification() {
    this.require_current();
    const capsule = this.#assembleFreshGhQualification();
    const capsulePath = `${this.control_root}/fresh-gh-capsule.py`;
    fs.writeFileSync(capsulePath, capsule.source, { flag: 'wx', mode: 0o600 });
    const result = await executeOwnedCapsule(this.#record.executable, capsule.source);
    this.require_current();
    const capsuleStat = fs.lstatSync(capsulePath);
    if (!capsuleStat.isFile() || capsuleStat.uid !== process.getuid() ||
        (capsuleStat.mode & 0o077) !== 0 ||
        digest(fs.readFileSync(capsulePath)) !== capsule.sourceSha256)
      throw new Error('Fresh GH original capsule changed');
    if (result?.state !== 'fresh-gh-native-positive' || result.authority !== false ||
        result.sealed_execution !== true) throw new Error('Fresh GH positive result');
    const receipt = { ...result, host: this.#record.host,
      original_sdk_sha256: this.#manifestDigest, capsule_sha256: capsule.sourceSha256,
      capsule_path: capsulePath, capsule_bytes: capsule.source.length,
      sources: capsule.manifest };
    const bytes = Buffer.from(JSON.stringify(receipt));
    const receiptPath = `${this.control_root}/fresh-gh-native-positive.json`;
    fs.writeFileSync(receiptPath, bytes, { flag: 'wx', mode: 0o600 });
    return { receiptPath, receiptSha256: digest(bytes), controlRoot: this.control_root };
  }
}
Object.freeze(FreshFoundationObservation.prototype);

async function executeOwnedCapsule(executable, source) {
  if (!Buffer.isBuffer(source) || source.length > 64 * 1024 * 1024)
    throw new Error('Fresh GH owned capsule bound');
  return await new Promise((resolve, reject) => {
    const child = spawn(executable, ['-I', '-S', '-B', '-'],
      { env: ENVIRONMENT, cwd: '/', shell: false, detached: true,
        stdio: ['pipe', 'pipe', 'pipe'] });
    const output = [];
    let size = 0;
    let failure = false;
    // Linux dedicated process group contains fork workers and the sealed GH
    // subprocess. Reap the entire group on every exit, including failures.
    const terminateGroup = () => {
      if (!child.pid) return;
      try { process.kill(-child.pid, 'SIGKILL'); }
      catch (error) { if (error.code !== 'ESRCH') failure = true; }
    };
    const timer = setTimeout(() => { failure = true; terminateGroup(); }, 900000);
    child.stdout.on('data', chunk => {
      size += chunk.length;
      if (size > 8 * 1024 * 1024) { failure = true; terminateGroup(); }
      else output.push(chunk);
    });
    // Child diagnostics may contain untrusted network/receipt fields. Bound and
    // discard them; no partial result is published on any failure.
    let errors = 0;
    child.stderr.on('data', chunk => {
      errors += chunk.length;
      if (errors > 1024 * 1024) { failure = true; terminateGroup(); }
    });
    child.once('error', () => { failure = true; });
    child.stdin.on('error', () => { failure = true; terminateGroup(); });
    child.once('close', (code, signal) => {
      clearTimeout(timer);
      terminateGroup();
      if (failure || code !== 0 || signal) return reject(new Error('Fresh GH native capsule failed'));
      try { resolve(JSON.parse(Buffer.concat(output).toString('utf8'))); }
      catch { reject(new Error('Fresh GH native capsule result')); }
    });
    child.stdin.end(source);
  });
}

// Zero arguments: no caller record, digest, interpreter, CA, or control path.
// The reviewed immutable workflow must call this as its first fresh-host step.
export function captureFreshFoundation() {
  if (process.platform !== 'linux' || !['x64', 'arm64'].includes(process.arch) ||
      process.env.GITHUB_ACTIONS !== 'true' || process.env.RUNNER_TEMP !== FIXED_TEMP ||
      !process.execPath.endsWith('/externals/node24/bin/node') ||
      Number(process.versions.node.split('.')[0]) !== 24)
    throw new Error('Foundation first-step SDK boundary');
  namespaceIdentity(FIXED_TEMP);
  fs.mkdirSync(CONTROL_PARENT, { mode: 0o700 });
  const controlRoot = fs.mkdtempSync(`${CONTROL_PARENT}/native-`);
  fs.chmodSync(controlRoot, 0o700);
  const observed = collectFoundationPython();
  const roots = observed.roots;
  const record = { ...observed, schema: 2,
    initial_trust: 'reviewed-immutable-first-step-fresh-runner-sdk',
    // The server-selected Node runtime is the external initial observer trust
    // primitive. Hashing it documents identity; this does not self-qualify it.
    observer: { authority: 'github-server-selected-node24-first-step',
      executable: fs.realpathSync(process.execPath), version: process.versions.node,
      sha256: digest(fs.readFileSync(process.execPath)) },
    roots, inventory: observed.inventory, sources: sourceManifest() };
  return new FreshFoundationObservation(ISSUER_TOKEN, record, controlRoot);
}

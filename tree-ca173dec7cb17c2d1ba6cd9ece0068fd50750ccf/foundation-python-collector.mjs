import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { inventoryFoundation, canonicalInventory } from './foundation-python-inventory.mjs';
import { foundationSharedLibraries, foundationLoaderSearchRoots, nativeLoaderRoots }
  from './foundation-native-loader.mjs';

// Qualification observation only. Run as first fresh-host action, before any
// checkout, caller helper, cache restoration, or downloaded tool execution.
const IMAGE_SOURCE = 'https://github.com/actions/runner-images/tree/6d942e630479cd99a93dadfc766af11242bfa402';
const ENVIRONMENT = Object.freeze({ LANG: 'C', LC_ALL: 'C' });
const INTERPRETERS = {
  'ubuntu26': '/usr/bin/python3.14',
  'ubuntu24': '/usr/bin/python3.12',
  'ubuntu22': '/usr/bin/python3.10'
};
const PROBE = `import sys, json, struct, os
print(json.dumps({"executable": os.path.realpath(sys.executable),
 "version": sys.version, "versionPair": list(sys.version_info[:2]),
 "platlibdir": sys.platlibdir, "path": sys.path,
 "flags": [sys.flags.isolated, sys.flags.no_site,
           sys.flags.ignore_environment, sys.flags.dont_write_bytecode],
 "struct": struct.__file__, "extension": getattr(sys.modules["_struct"], "__file__", None),
 "extensionOrigin": sys.modules["_struct"].__spec__.origin,
 "startup": sorted((name, getattr(module, "__file__", None))
                   for name, module in sys.modules.items())}))`;

function invoke(executable, argv) {
  const result = spawnSync(executable, argv, {
    env: ENVIRONMENT, cwd: '/', timeout: 30000, maxBuffer: 8 * 1024 * 1024,
    encoding: 'utf8', shell: false
  });
  if (result.error || result.status !== 0) throw new Error('foundation observation failed');
  return result.stdout;
}

function closureRoots(observation, executable) {
  const roots = new Set([executable, ...[observation.extension].filter(value => value !== null)]);
  for (const directory of [path.dirname(executable), path.dirname(path.dirname(executable))]) {
    roots.add(path.join(directory, 'pyvenv.cfg'));
    roots.add(path.join(directory, 'python._pth'));
    roots.add(path.join(directory, 'python3._pth'));
    roots.add(path.join(directory, `${path.basename(executable)}._pth`));
  }
  roots.add(path.join(path.dirname(executable), 'pybuilddir.txt'));
  roots.add(path.join(path.dirname(executable), 'Modules', 'Setup.local'));
  // getpath's executable-relative search precedes compiled default prefixes.
  // Missing landmarks must remain absent, not merely the resulting sys.path.
  const version = observation.versionPair.join('.');
  let prefix = path.dirname(executable);
  for (;;) {
    for (const landmark of [`${observation.platlibdir}/python${version}/os.py`,
      `${observation.platlibdir}/python${version}/os.pyc`,
      `${observation.platlibdir}/python${version}/lib-dynload`,
      `${observation.platlibdir}/python${observation.versionPair.join('')}.zip`])
      roots.add(path.join(prefix, landmark));
    if (prefix === '/') break;
    prefix = path.dirname(prefix);
  }
  if (process.platform === 'linux') {
    // The native loader consults these without environment variables. Absent
    // preload/config paths are inventoried too, preventing later insertion.
    for (const filename of ['/etc/ld.so.cache', '/etc/ld.so.preload',
      '/etc/ld.so.conf', '/etc/ld.so.conf.d']) roots.add(filename);
  }
  for (const entry of observation.path) {
    if (!path.isAbsolute(entry)) throw new Error('relative foundation import path');
    roots.add(entry);
  }
  for (const [, filename] of observation.startup) {
    if (filename === null) continue;
    if (typeof filename !== 'string' || !path.isAbsolute(filename))
      throw new Error('relative foundation startup module');
    roots.add(filename);
  }
  const extensions = inventoryFoundation([...roots]).filter(record =>
    ['file', 'symlink'].includes(record.kind) && /\.(?:so|dylib)$/.test(record.path))
    .map(record => record.path);
  const pending = [executable, ...[observation.extension].filter(value => value !== null), ...extensions];
  const inspected = new Set();
  while (pending.length) {
    const binary = pending.pop();
    if (inspected.has(binary)) continue;
    inspected.add(binary);
    roots.add(`${binary}._pth`);
    roots.add(`${fs.realpathSync(binary)}._pth`);
    for (const directory of foundationLoaderSearchRoots(binary)) roots.add(directory);
    for (const library of foundationSharedLibraries(binary)) {
      // macOS system dylibs reside in the signed OS shared cache, not standalone
      // filesystem objects. Their authority still needs a hosted cache record.
      if (!fs.existsSync(library)) throw new Error('unobservable foundation shared cache');
      roots.add(library);
      pending.push(library);
    }
  }
  // All native extensions above, including unimported ones, were traversed.
  return [...roots].sort();
}

function initialPythonScope(executable, version) {
  const roots = new Set([executable, ...nativeLoaderRoots()]);
  const pythonTrees = [];
  let prefix = path.dirname(executable);
  for (;;) {
    for (const library of ['lib', 'lib64']) {
      const tree = path.join(prefix, library, `python${version}`);
      roots.add(tree); pythonTrees.push(tree);
      for (const landmark of ['os.py', 'os.pyc', 'lib-dynload'])
        roots.add(path.join(tree, landmark));
      roots.add(path.join(prefix, library, `python${version.replace('.', '')}.zip`));
    }
    if (prefix === '/') break;
    prefix = path.dirname(prefix);
  }
  const markers = new Set([
    ...[path.dirname(executable), path.dirname(path.dirname(executable))].flatMap(directory =>
      ['pyvenv.cfg', 'python._pth', 'python3._pth', `${path.basename(executable)}._pth`]
        .map(name => path.join(directory, name))),
    path.join(path.dirname(executable), 'pybuilddir.txt'),
    path.join(path.dirname(executable), 'Modules', 'Setup.local')
  ]);
  const treeInventory = inventoryFoundation(pythonTrees);
  const pending = [executable, ...treeInventory.filter(item =>
    item.kind === 'file' && /\.so$/.test(item.path)).map(item => item.path)];
  const inspected = new Set();
  while (pending.length) {
    const binary = pending.pop();
    if (inspected.has(binary)) continue;
    inspected.add(binary); roots.add(binary);
    markers.add(`${binary}._pth`); markers.add(`${fs.realpathSync(binary)}._pth`);
    for (const directory of foundationLoaderSearchRoots(binary)) roots.add(directory);
    for (const library of foundationSharedLibraries(binary)) {
      roots.add(library); pending.push(library);
    }
  }
  for (const marker of markers) {
    if (fs.existsSync(marker)) throw new Error('unsupported original Python path configuration');
    roots.add(marker);
  }
  const scope = [...roots].sort();
  return { roots: scope, inventory: inventoryFoundation(scope) };
}

function assertOriginallyCaptured(paths, original) {
  const inventory = new Map(original.inventory.map(item => [item.path, item]));
  for (const filename of paths) {
    if (typeof filename !== 'string' || !path.isAbsolute(filename) ||
        path.normalize(filename) !== filename || !inventory.has(filename))
      throw new Error('Python path outside original source scope');
  }
  if (canonicalInventory(inventoryFoundation(original.roots)) !==
      canonicalInventory(original.inventory)) throw new Error('original SDK changed');
}

export function collectFoundationPython() {
  if (process.platform !== 'linux') throw new Error('unsupported first-exec Foundation platform');
  const selected = INTERPRETERS[process.env.ImageOS];
  if (!selected) throw new Error('unsupported foundation image');
  const executable = fs.realpathSync(selected);
  if (executable !== selected) throw new Error('unsupported original interpreter alias');
  const version = path.basename(selected).slice('python'.length);
  // Source-qualified Ubuntu apt Python has default /usr prefixes. Every
  // executable-relative alternative, import tree, codec and native loader scope
  // is captured before its first instruction; alternate path configs are cold.
  const original = initialPythonScope(executable, version);
  const observation = JSON.parse(invoke(executable, ['-I', '-S', '-B', '-c', PROBE]));
  if (observation.executable !== executable ||
      JSON.stringify(observation.versionPair) !== JSON.stringify(version.split('.').map(Number)) ||
      !['lib', 'lib64'].includes(observation.platlibdir) ||
      (observation.extension === null && observation.extensionOrigin !== 'built-in') ||
      JSON.stringify(observation.flags) !== '[1,1,1,1]')
    throw new Error('unqualified foundation startup configuration');
  assertOriginallyCaptured([...observation.path, ...[observation.extension].filter(value => value !== null),
    ...observation.startup.map(([, filename]) => filename).filter(filename => filename !== null)], original);
  assertOriginallyCaptured(closureRoots(observation, executable), original);
  return { schema: 1, authority: false, imageSource: IMAGE_SOURCE,
    host: { platform: process.platform, architecture: process.arch,
      imageOS: process.env.ImageOS ?? null, imageVersion: process.env.ImageVersion ?? null },
    executable, argv: ['-I', '-S', '-B'], environment: ENVIRONMENT,
    observation, pre_python_capture: true, ...original };
}

if (process.argv[1] && fs.existsSync(process.argv[1]) &&
    fs.realpathSync(process.argv[1]) === fs.realpathSync(import.meta.filename)) {
  try { process.stdout.write(JSON.stringify(collectFoundationPython()) + '\n'); }
  catch { process.stderr.write('Foundation Python closure remains unqualified.\n'); process.exitCode = 1; }
}

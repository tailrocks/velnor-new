import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { opensslConfigurationRoots } from './foundation-native-openssl.mjs';

const ENVIRONMENT = Object.freeze({ LANG: 'C', LC_ALL: 'C' });
function invoke(executable, argv) {
  const result = spawnSync(executable, argv, {
    env: ENVIRONMENT, cwd: '/', timeout: 30000, maxBuffer: 8 * 1024 * 1024,
    encoding: 'utf8', shell: false
  });
  if (result.error || result.status !== 0) throw new Error('foundation observation failed');
  return result.stdout;
}

export function foundationSharedLibraries(binary) {
  if (process.platform === 'linux') {
    const listing = invoke('/usr/bin/ldd', [binary]);
    if (/not found/.test(listing)) throw new Error('unresolved foundation loader');
    const libraries = [];
    for (const line of listing.trim().split('\n').map(value => value.trim())) {
      if (/^linux-vdso\.so\.\d+ \(0x[0-9a-f]+\)$/.test(line) ||
          line === 'statically linked') continue;
      const match = line.match(/^(?:\S+ => )?(\/\S+) \(0x[0-9a-f]+\)$/);
      if (!match) throw new Error('unrecognized foundation loader output');
      libraries.push(match[1]);
    }
    return libraries;
  }
  const listing = invoke('/usr/bin/otool', ['-L', binary]);
  const references = listing.split('\n').slice(1).filter(line => line.trim())
    .map(line => line.trim().split(' (')[0]);
  for (const reference of references)
    if (!path.isAbsolute(reference)) throw new Error('unresolved foundation loader');
  return references;
}

export function foundationLoaderSearchRoots(binary) {
  if (process.platform !== 'linux') return [];
  const listing = invoke('/usr/bin/readelf', ['-d', binary]);
  if (/\((?:AUDIT|DEPAUDIT|FILTER|AUXILIARY)\)/.test(listing))
    throw new Error('unsupported foundation loader mechanism');
  const roots = [];
  for (const match of listing.matchAll(/\((?:RUNPATH|RPATH)\)[^\n]*\[([^\]]*)\]/g)) {
    for (const entry of match[1].split(':')) {
      for (const origin of [path.dirname(binary), path.dirname(fs.realpathSync(binary))]) {
        const resolved = entry.replace(/\$\{ORIGIN\}|\$ORIGIN/g, origin);
        // Seal both alias and actual origins; unknown variables and relative
        // components cannot be proved by currently resolved dependencies.
        if (!path.isAbsolute(resolved) || resolved.includes('$'))
          throw new Error('unqualified foundation loader search path');
        roots.push(path.normalize(resolved));
      }
    }
  }
  return roots;
}

const MAX_CONFIG_BYTES = 1024 * 1024;
const MAX_CONFIG_FILES = 1024;
const OBSERVER_TOOLS = Object.freeze([
  '/usr/bin/ldd', '/usr/bin/readelf', '/bin/sh', '/bin/bash', '/usr/bin/bash'
]);
export const FOUNDATION_CA_FILE = '/etc/ssl/certs/ca-certificates.crt';
export const FOUNDATION_ARCHIVE_ALIASES = Object.freeze([
  '/usr/bin/gzip', '/bin/gzip', '/usr/bin/tar', '/bin/tar'
]);

// First-step fresh-image observation, not a registry of caller-approved paths.
// Full directories include glibc-hwcaps and dynamically loaded native plugins.
export function nativeLoaderRoots() {
  if (process.platform !== 'linux' || !['x64', 'arm64'].includes(process.arch))
    throw new Error('native Foundation qualification requires supported Linux');
  const roots = new Set([
    '/lib', '/lib64', '/usr/lib', '/usr/lib64', '/usr/local/lib',
    '/etc/ld.so.cache', '/etc/ld.so.conf', '/etc/ld.so.conf.d', '/etc/ld.so.preload',
    '/etc/ssl/certs', '/usr/share/ca-certificates', FOUNDATION_CA_FILE,
    ...opensslConfigurationRoots(),
    ...OBSERVER_TOOLS,
    '/etc/nsswitch.conf', '/etc/resolv.conf', '/etc/hosts',
    ...FOUNDATION_ARCHIVE_ALIASES
  ]);
  const seen = new Set();
  function readConfig(filename) {
    if (seen.has(filename)) return;
    if (seen.size >= MAX_CONFIG_FILES) throw new Error('Foundation loader config bound');
    seen.add(filename);
    roots.add(filename);
    if (!fs.existsSync(filename)) return;
    const resolved = fs.realpathSync(filename);
    const stat = fs.statSync(resolved);
    if (!stat.isFile() || stat.size > MAX_CONFIG_BYTES)
      throw new Error('Foundation loader config bound');
    for (const raw of fs.readFileSync(resolved, 'utf8').split('\n')) {
      const line = raw.split('#')[0].trim();
      if (!line) continue;
      if (line.startsWith('include ')) {
        const pattern = line.slice(8).trim();
        if (!path.isAbsolute(pattern) || pattern.includes('\0'))
          throw new Error('Foundation loader include path');
        // Capture the full include directory so inserting a newly matching
        // config cannot preserve the observation manifest.
        const firstWildcard = pattern.search(/[?*[]/);
        const prefix = firstWildcard < 0 ? pattern : pattern.slice(0, firstWildcard);
        const directory = prefix.endsWith('/') ? prefix.slice(0, -1) : path.dirname(prefix);
        roots.add(directory);
        let count = 0;
        for (const included of fs.globSync(pattern)) {
          if (++count > MAX_CONFIG_FILES) throw new Error('Foundation include bound');
          readConfig(included);
        }
      } else {
        if (!path.isAbsolute(line) || path.normalize(line) !== line)
          throw new Error('Foundation loader search path');
        roots.add(line);
      }
    }
  }
  readConfig('/etc/ld.so.conf');
  const pending = [...FOUNDATION_ARCHIVE_ALIASES, ...OBSERVER_TOOLS];
  const inspected = new Set();
  while (pending.length) {
    const binary = pending.pop();
    if (inspected.has(binary)) continue;
    inspected.add(binary);
    roots.add(binary);
    const descriptor = fs.openSync(binary, fs.constants.O_RDONLY);
    const header = Buffer.alloc(12);
    try { fs.readSync(descriptor, header, 0, header.length, 0); }
    finally { fs.closeSync(descriptor); }
    if (!header.subarray(0, 4).equals(Buffer.from([0x7f, 0x45, 0x4c, 0x46]))) {
      // The fresh glibc ldd observer is a fixed Bash script, not an ELF image.
      if (binary !== '/usr/bin/ldd' || !header.equals(Buffer.from('#!/bin/bash\n')))
        throw new Error('Foundation observer executable format');
      pending.push('/bin/bash');
      continue;
    }
    for (const directory of foundationLoaderSearchRoots(binary)) roots.add(directory);
    for (const dependency of foundationSharedLibraries(binary)) {
      roots.add(dependency);
      pending.push(dependency);
    }
  }
  if (fs.existsSync('/etc/ld.so.preload') &&
      fs.readFileSync('/etc/ld.so.preload', 'utf8').split('\n')
        .some(line => line.split('#')[0].trim()))
    throw new Error('Foundation preload is unsupported');
  return [...roots].sort();
}

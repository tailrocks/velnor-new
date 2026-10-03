import { execFileSync, spawn } from 'node:child_process';
import {
  accessSync,
  constants as fsConstants,
  createWriteStream,
  rmSync,
  writeFileSync
} from 'node:fs';
import { pipeline } from 'node:stream/promises';
import path from 'node:path';
import { admitArchive, createQuarantinePath, extractAdmittedArchive, validateQuarantinePath, writeAdmissionManifest, ArchiveConfigurationError } from './archive-admission.mjs';
import { createArchiveEncoder } from './archive-codec.mjs';
import { validateArchiveOutput } from './archive-output.mjs';
import * as utils from './cacheUtils.js';
import { CompressionMethod, ArchiveToolType, ManifestFilename } from './constants.js';

const ownedArchiveVersion = 'velnor-cache-archive-v1';
if (!['linux', 'darwin'].includes(process.platform)) {
  throw new Error(`${ownedArchiveVersion}: unsupported platform ${process.platform}`);
}

function archivePathEntries() {
  return ['/usr/bin', '/bin', '/usr/sbin', '/sbin'];
}

function archiveEnvironment() {
  return {
    PATH: archivePathEntries().join(path.delimiter),
    LANG: 'C',
    LC_ALL: 'C',
    COPYFILE_DISABLE: '1'
  };
}

function findArchiveExecutable() {
  const candidate = '/usr/bin/tar';
  try {
    accessSync(candidate, fsConstants.X_OK);
    return candidate;
  }
  catch (_error) {
    return undefined;
  }
}

function readArchiveVersion(executable) {
  return execFileSync(executable, ['--version'], {
    encoding: 'utf8',
    env: archiveEnvironment(),
    stdio: ['ignore', 'pipe', 'pipe']
  });
}

export async function getTarPath() {
  const executable = findArchiveExecutable();
  if (!executable) throw new Error(`${ownedArchiveVersion}: qualified tar executable not found`);
  const version = readArchiveVersion(executable).toLowerCase();
  if (process.platform === 'darwin' && !version.includes('bsdtar')) {
    throw new Error(`${ownedArchiveVersion}: expected BSD tar on macOS`);
  }
  if (process.platform === 'linux' && !version.includes('gnu tar')) {
    throw new Error(`${ownedArchiveVersion}: expected GNU tar on Linux`);
  }
  return { path: executable, type: process.platform === 'darwin' ? ArchiveToolType.BSD : ArchiveToolType.GNU };
}

function getWorkingDirectory() {
  return process.env.GITHUB_WORKSPACE || process.cwd();
}

function getTarArgs(tarPath, compressionMethod, type, archivePath, manifestPath) {
  if (![CompressionMethod.Gzip, CompressionMethod.ZstdWithoutLong].includes(compressionMethod)) {
    throw new Error(`${ownedArchiveVersion}: unsupported compression method ${compressionMethod}`);
  }
  const args = [];
  const workingDirectory = getWorkingDirectory();
  if (type === 'create') {
    args.push('--posix', '-cf', archivePath, '-P', '-C', workingDirectory, '--null', '--files-from', manifestPath);
    if (compressionMethod === CompressionMethod.Gzip) args.push('-z');
  }
  else {
    throw new Error(`${ownedArchiveVersion}: native archive extraction is not an admission path`);
  }
  args.push('--no-xattrs', '--no-acls');
  if (tarPath.type === ArchiveToolType.BSD) args.push('--no-fflags', '--no-mac-metadata');
  return args;
}

function processFailure(child, command) {
  return new Promise((resolve, reject) => {
    let stderr = '';
    child.stderr.on('data', chunk => {
      if (stderr.length < 8192) stderr += chunk.toString('utf8').slice(0, 8192 - stderr.length);
    });
    child.once('error', reject);
    child.once('close', (code, signal) => {
      if (code === 0) resolve();
      else reject(new Error(`${command} failed (${code ?? signal}): ${stderr}`));
    });
  });
}

async function createZstdArchive(tarPath, archiveFolder, args, outputPath) {
  const encoder = createArchiveEncoder(CompressionMethod.ZstdWithoutLong);
  const child = spawn(tarPath.path, args, {
    cwd: archiveFolder,
    env: archiveEnvironment(),
    stdio: ['ignore', 'pipe', 'pipe']
  });
  try {
    await Promise.all([
      pipeline(child.stdout, encoder, createWriteStream(outputPath, { mode: 0o600 })),
      processFailure(child, tarPath.path)
    ]);
  }
  catch (error) {
    child.kill('SIGTERM');
    rmSync(outputPath, { force: true });
    throw error;
  }
}

async function createGzipArchive(tarPath, archiveFolder, args) {
  const child = spawn(tarPath.path, args, {
    cwd: archiveFolder,
    env: archiveEnvironment(),
    stdio: ['ignore', 'ignore', 'pipe']
  });
  await processFailure(child, tarPath.path);
}

function archiveManifestPaths(sourceDirectories) {
  if (!Array.isArray(sourceDirectories) || sourceDirectories.length === 0) {
    throw new Error(`${ownedArchiveVersion}: source directories are required`);
  }
  const workspace = getWorkingDirectory();
  return sourceDirectories.map(sourceDirectory => {
    if (typeof sourceDirectory !== 'string' || sourceDirectory.length === 0 || sourceDirectory.includes('\0')) {
      throw new Error(`${ownedArchiveVersion}: source directory is invalid`);
    }
    const absolute = path.resolve(workspace, sourceDirectory);
    const relative = path.relative(workspace, absolute).replaceAll(path.sep, '/');
    return relative || '.';
  });
}

export async function extractTar(archivePath, compressionMethod, admissionOptions) {
  if (!admissionOptions || typeof admissionOptions.quarantinePath !== 'string') {
    throw new ArchiveConfigurationError('QUARANTINE_INPUT', 'restore quarantinePath is required');
  }
  if (!Array.isArray(admissionOptions.roots) || admissionOptions.roots.length === 0) {
    throw new ArchiveConfigurationError('QUARANTINE_INPUT', 'restore roots are required for quarantine admission');
  }
  const quarantinePath = validateQuarantinePath(admissionOptions.quarantinePath);
  const manifest = await admitArchive(archivePath, compressionMethod, admissionOptions.roots);
  const createdPath = createQuarantinePath(quarantinePath, path.basename(quarantinePath));
  const extracted = await extractAdmittedArchive(archivePath, manifest, createdPath);
  const manifestPath = writeAdmissionManifest(createdPath, extracted);
  return { quarantinePath: createdPath, manifestPath, manifest: extracted };
}

export async function createTar(archiveFolder, sourceDirectories, compressionMethod) {
  const manifestPaths = archiveManifestPaths(sourceDirectories);
  validateArchiveOutput(archiveFolder, sourceDirectories, getWorkingDirectory(),
    [ManifestFilename, utils.getCacheFileName(compressionMethod)]);
  writeFileSync(path.join(archiveFolder, ManifestFilename), `${manifestPaths.join('\0')}\0`, { mode: 0o600 });
  const tarPath = await getTarPath();
  const outputPath = path.join(archiveFolder, utils.getCacheFileName(compressionMethod));
  const archivePath = compressionMethod === CompressionMethod.ZstdWithoutLong ? '-' : utils.getCacheFileName(compressionMethod);
  const args = getTarArgs(tarPath, compressionMethod, 'create', archivePath, path.join(archiveFolder, ManifestFilename));
  if (compressionMethod === CompressionMethod.ZstdWithoutLong) await createZstdArchive(tarPath, archiveFolder, args, outputPath);
  else await createGzipArchive(tarPath, archiveFolder, args);
}

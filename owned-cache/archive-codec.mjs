import { createReadStream, statSync } from 'node:fs';
import { Transform } from 'node:stream';
import * as zlib from 'node:zlib';

const archiveVersion = 'velnor-cache-archive-v1';
const zstdWindowLog = 30;
const zstdWindowParam = zlib.constants.ZSTD_d_windowLogMax;

function requireNode24() {
  const major = Number(process.versions.node.split('.')[0]);
  if (major !== 24) throw codecError('ARCHIVE_CODEC', 'Node 24 is required for the archive codec');
}

function codecError(code, message) {
  const error = new Error(`${archiveVersion}: ${message}`);
  error.code = code;
  return error;
}

function requireCodec(name, codec) {
  requireNode24();
  if (typeof codec !== 'function' || typeof zstdWindowParam !== 'number') {
    throw codecError('ARCHIVE_CODEC', `Node ${name} codec is unavailable`);
  }
  return codec;
}

function zstdOptions() {
  return { params: { [zstdWindowParam]: zstdWindowLog } };
}

export function createArchiveDecoder(compressionMethod) {
  if (compressionMethod === 'gzip') return zlib.createGunzip();
  if (compressionMethod === 'zstd' || compressionMethod === 'zstd-without-long') {
    const decoder = requireCodec('zstd decompression', zlib.createZstdDecompress);
    return decoder(zstdOptions());
  }
  throw codecError('ARCHIVE_CODEC', `unsupported decoder ${compressionMethod}`);
}

export function createArchiveEncoder(compressionMethod) {
  if (compressionMethod !== 'zstd-without-long') {
    throw codecError('ARCHIVE_CODEC', `unsupported encoder ${compressionMethod}`);
  }
  const encoder = requireCodec('zstd compression', zlib.createZstdCompress);
  return encoder();
}

export function decodeArchive(file, compressionMethod, limits) {
  const source = createReadStream(file, { highWaterMark: 1024 * 1024 });
  const decoder = createArchiveDecoder(compressionMethod);
  const state = { compressedBytes: statSync(file).size, decodedBytes: 0 };
  const bounded = new Transform({
    transform(chunk, _encoding, callback) {
      state.decodedBytes += chunk.length;
      if (state.decodedBytes > limits.maxDecodedBytes) {
        callback(codecError('ARCHIVE_DECODE_LIMIT', 'decoded archive exceeds maximum size'));
        return;
      }
      if (state.compressedBytes > 0 && state.decodedBytes / state.compressedBytes > limits.maxRatio) {
        callback(codecError('ARCHIVE_DECODE_RATIO', 'archive decompression ratio exceeds maximum'));
        return;
      }
      callback(null, chunk);
    }
  });
  const abort = error => {
    decoder.destroy(error);
    bounded.destroy(error);
  };
  source.on('error', abort);
  decoder.on('error', error => bounded.destroy(error));
  bounded.on('error', error => source.destroy(error));
  source.pipe(decoder).pipe(bounded);
  return { source, stream: bounded, state };
}

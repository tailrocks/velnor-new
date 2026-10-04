"""Pure source producer: official pinned cacache only; no project execution."""
import platform
import stat
import subprocess
import sys
from pathlib import Path

class SourceEligibilityDenied(Exception):
    def __init__(self, reason):
        self.reason = reason


class NativeSourceFailure(Exception):
    reason = 'native_store_failure'


class NativeOwnerFailure(NativeSourceFailure):
    reason = 'tool_owner_failure'


class NativeIntegrityFailure(NativeSourceFailure):
    reason = 'source_integrity_failure' 


NATIVE = r"""
const fs = require('fs');
const path = require('path');
const cp = require('child_process');
async function main() {
  const operation = process.argv[1];
  if (operation === 'owner') {
    const npm = path.join(path.dirname(process.execPath), 'npm');
    const root = cp.execFileSync(npm, ['--userconfig=/dev/null',
      '--globalconfig=' + process.env.npm_config_globalconfig, 'root', '--global'], {encoding:'utf8'}).trim();
    const packageRoot = path.join(root, 'npm');
    const version = require(path.join(packageRoot, 'package.json')).version;
    process.stdout.write(JSON.stringify({node:process.version, npm:version,
      module_abi:process.versions.modules,
      module:path.join(packageRoot, 'node_modules', 'cacache')}));
    return;
  }
  const cacache = require(process.env.VELNOR_NPM_CACACHE_MODULE);
  const cache = process.env.npm_config_cache + '/_cacache';
  const integrity = process.argv[2];
  if (operation === 'read') {
    const chunks = [];
    let size = 0;
    const stream = cacache.get.stream.byDigest(cache, integrity);
    for await (const chunk of stream) {
      size += chunk.length;
      if (size > 67108864) throw new Error('native content exceeds bound');
      chunks.push(chunk);
    }
    process.stdout.write(Buffer.concat(chunks));
  } else if (operation === 'write') {
    const data = fs.readFileSync(0);
    if (data.length > 67108864) throw new Error('source exceeds bound');
    await cacache.rm.content(cache, integrity);
    await cacache.put(cache, integrity, data, {integrity, algorithms:['sha512']});
    const verified = await cacache.get.byDigest(cache, integrity);
    if (!verified.equals(data)) throw new Error('native replacement verification failed');
  } else {
    throw new Error('unknown native operation');
  }
}
main().catch(() => { process.exitCode = 1; });
"""


class NativeStore:
    def __init__(self, cache, node_bin, compatibility, expected_owner):
        if not isinstance(compatibility, str) or len(compatibility) != 64 or any(
                char not in '0123456789abcdef' for char in compatibility):
            raise ValueError('unqualified producer source body')
        if (not isinstance(expected_owner, dict)
                or set(expected_owner) != {'node', 'npm', 'module_abi'}
                or any(not isinstance(value, str) or not value for value in expected_owner.values())):
            raise ValueError('unqualified native owner expectation')
        self.lock = threading.Lock()
        self.cache = Path(cache)
        self.node_bin = Path(node_bin)
        if not self.cache.is_absolute() or not self.node_bin.is_absolute():
            raise ValueError('native owner requires absolute paths')
        self.cache.mkdir(parents=True, exist_ok=True)
        global_config = self.cache / 'empty-global.npmrc'
        config_fd = os.open(global_config, os.O_WRONLY | os.O_CREAT | os.O_NOFOLLOW, 0o600)
        try:
            info = os.fstat(config_fd)
            if not stat.S_ISREG(info.st_mode) or info.st_nlink != 1:
                raise ValueError('redirected npm configuration')
            os.ftruncate(config_fd, 0)
        finally:
            os.close(config_fd)
        self.env = {'PATH': str(self.node_bin.parent) + ':/usr/bin:/bin',
                    'HOME': str(self.cache.parent), 'npm_config_cache': str(self.cache),
                    'npm_config_globalconfig': str(global_config)}
        try:
            owner = json.loads(self.run('owner'))
        except (OSError, ValueError, subprocess.SubprocessError):
            raise NativeOwnerFailure('native npm owner unavailable') from None
        if any(owner.get(key) != value for key, value in expected_owner.items()):
            raise NativeOwnerFailure('unsupported native npm owner')
        self.env['VELNOR_NPM_CACACHE_MODULE'] = owner['module']
        self.identity = {'schema': 'velnor-npm-public-proof-v1',
                         **expected_owner,
                         'platform': sys.platform, 'machine': platform.machine(),
                         'source_body_sha256': compatibility}

    def run(self, operation, integrity=None, data=None, deadline=None):
        args = [str(self.node_bin), '-e', NATIVE, operation]
        if integrity is not None:
            args.append(integrity)
        timeout = 10 if deadline is None else min(10, max(.1, deadline - time.monotonic()))
        result = subprocess.run(args, input=data, stdout=subprocess.PIPE,
                                stderr=subprocess.DEVNULL, env=self.env,
                                cwd='/', timeout=timeout, check=True)
        if len(result.stdout) > MAX_TARBALL:
            raise ValueError('native read exceeds bound')
        return result.stdout

    def read(self, integrity, deadline):
        with self.lock:
            return self.run('read', integrity, deadline=deadline)

    def write(self, integrity, data, deadline):
        try:
            with self.lock:
                self.run('write', integrity, data, deadline)
        except (OSError, ValueError, subprocess.SubprocessError):
            raise NativeSourceFailure('native cache write verification failed') from None


def source_identity(source):
    if set(source) != {'name', 'version', 'resolved', 'integrity'}:
        raise ValueError('invalid source tuple fields')
    return json.dumps(source, sort_keys=True, separators=(',', ':'), ensure_ascii=True)


def cache_directory(cache):
    directory = Path(cache)
    if not directory.is_absolute():
        raise ValueError('nonabsolute producer cache')
    for ancestor in [*reversed(directory.parents), directory]:
        if ancestor.is_symlink():
            raise ValueError('redirected producer cache')
        ancestor.mkdir(exist_ok=True)
        if not ancestor.is_dir():
            raise ValueError('invalid producer cache')
    return directory


def read_provenance(directory, identity):
    try:
        path = directory / 'public-proof-v1.json'
        info = path.lstat()
        if not stat.S_ISREG(info.st_mode) or info.st_nlink != 1 or info.st_size > 8 * MAX_METADATA:
            return set()
        marker = json.loads(path.read_bytes())
        if not isinstance(marker, dict) or set(marker) != {'owner', 'sources'}:
            return set()
        if marker['owner'] != identity or not isinstance(marker['sources'], list) or len(marker['sources']) > MAX_PACKAGES:
            return set()
        return {source_identity(source) for source in marker['sources']}
    except (OSError, ValueError, TypeError):
        return set()


def produce(sources, cache, node_bin, expected_owner, transport=open_public, report=None, compatibility=None):
    if not isinstance(sources, list) or len(sources) > MAX_PACKAGES:
        raise ValueError('invalid source candidate bound')
    sources = [json.loads(value) for value in sorted({source_identity(source) for source in sources})]
    directory = cache_directory(cache)
    store = NativeStore(directory, node_bin, compatibility, expected_owner)
    prior = read_provenance(directory, store.identity)
    budget = Budget()
    reasons = set()
    reasons_lock = threading.Lock()

    def deny(reason):
        with reasons_lock:
            reasons.add(reason)
        return None

    def prepare(source):
        try:
            canonical = source_identity(source)
            expected = verify_metadata(source, budget, transport)
            data = None
            if canonical in prior:
                try:
                    data = store.read(source['integrity'], budget.deadline)
                except (OSError, ValueError, subprocess.SubprocessError):
                    pass
            if data is None or hashlib.sha512(data).digest() != expected:
                data = fetch(source['resolved'], MAX_TARBALL, budget, transport, tarball=True)
                if hashlib.sha512(data).digest() != expected:
                    raise NativeIntegrityFailure('public source integrity mismatch')
                store.write(source['integrity'], data, budget.deadline)
            return source
        except PublicSourceDenied:
            return deny('PRIVATE_OR_AUTH_REQUIRED')
        except UnsupportedRegistry:
            return deny('UNSUPPORTED_REGISTRY')
        except (OSError, TimeoutError, ValueError, http.client.HTTPException, PublicSourceUnavailable):
            return deny('PUBLIC_AUTHORITY_UNAVAILABLE')

    with concurrent.futures.ThreadPoolExecutor(max_workers=4) as workers:
        qualified = [source for source in workers.map(prepare, sources) if source is not None]
    if report is not None:
        report.update(source_count=len(sources), qualified_count=len(qualified),
                      metadata_bytes=budget.metadata_bytes, tarball_bytes=budget.tarball_bytes)
    if len(qualified) != len(sources):
        reason = next(reason for reason in ('PRIVATE_OR_AUTH_REQUIRED', 'UNSUPPORTED_REGISTRY',
            'PUBLIC_AUTHORITY_UNAVAILABLE') if reason in reasons)
        raise SourceEligibilityDenied(reason)
    marker = {'owner': store.identity, 'sources': sorted(qualified, key=source_identity)}
    destination = directory / 'public-proof-v1.json'
    temporary = directory / 'public-proof-v1.pending'
    descriptor = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    with os.fdopen(descriptor, 'w', encoding='utf-8') as output:
        json.dump(marker, output, sort_keys=True, separators=(',', ':'))
    os.replace(temporary, destination)
    integrities = sorted({source['integrity'] for source in qualified})
    print('npm public proof: %d integrities; %d metadata bytes; %d tarball bytes'
          % (len(integrities), budget.metadata_bytes, budget.tarball_bytes))
    return integrities


def main():
    try:
        sources = [json.loads(argument) for argument in sys.argv[5:]]
        if not sources:
            raise SourceEligibilityDenied('PUBLIC_AUTHORITY_UNAVAILABLE')
        report = {}
        qualified = produce(sources, os.environ['npm_config_cache'], sys.argv[1],
                            json.loads(sys.argv[4]), report=report, compatibility=sys.argv[3])
        os.environ['VELNOR_NPM_PUBLIC_INTEGRITIES'] = '\n'.join(qualified)
        os.environ['VELNOR_NPM_PUBLIC_PROOF_SAFE'] = 'true'
        exec(SANITIZER_CODE, globals())
        with open(os.environ['GITHUB_OUTPUT'], 'a', encoding='utf-8') as output:
            state = 'true' if qualified else 'false'
            output.write('cache_available=' + state + '\nverified=' + state + '\n')
            output.write('sourceidentity=' + sys.argv[2] + '\n')
            output.write('error=NONE\n')
            for field in ['source_count', 'qualified_count', 'metadata_bytes', 'tarball_bytes']:
                output.write(field + '=' + str(report[field]) + '\n')
    except Exception as error:
        # Eligibility denial is explicit; native verification failure remains red.
        with open(os.environ['GITHUB_OUTPUT'], 'a', encoding='utf-8') as output:
            output.write('cache_available=false\nverified=false\n')
            reason = error.reason if isinstance(error, SourceEligibilityDenied) else 'SOURCE_VERIFICATION_FAILED'
            output.write('error=' + reason + '\n')
            output.write('sourceidentity=' + sys.argv[2] + '\n')
        if not isinstance(error, SourceEligibilityDenied):
            raise SystemExit(1)

"""Immutable, bounded GitHub artifact transport for the fixed native APT jobs."""
import hashlib
import io
import json
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import sys
import tempfile
import threading
import zipfile


MAX_ARCHIVE = 512 * 1024 * 1024
MAX_CONTENT = 1024 * 1024 * 1024
MAX_FILE = 256 * 1024 * 1024
MAX_ENTRIES = 4096
MAX_JSON = 4 * 1024 * 1024
JOBS = {'incoming': 'Verify apt feed', 'staging': 'Stage apt feed'}
ROOTS = {'incoming': 'incoming', 'staging': 'public'}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def pairs(items):
    result = {}
    for key, value in items:
        require(key not in result, 'duplicate API JSON field')
        result[key] = value
    return result


def checksum(value, *, rest=False):
    require(isinstance(value, str), 'artifact digest required')
    pattern = 'sha256:([a-f0-9]{64})' if rest else '([a-f0-9]{64})'
    match = re.fullmatch(pattern, value)
    require(match is not None, 'invalid artifact SHA256')
    return match.group(1)


def context(kind):
    require(kind in JOBS, 'unknown artifact kind')
    repo = os.environ.get('GITHUB_REPOSITORY', '')
    require(re.fullmatch('[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+', repo) is not None,
            'invalid GitHub repository')
    require(all(part not in ('.', '..') for part in repo.split('/')), 'invalid GitHub repository')
    require(os.environ.get('GITHUB_SERVER_URL', 'https://github.com') == 'https://github.com',
            'GitHub.com required')
    require(os.environ.get('GITHUB_API_URL', 'https://api.github.com') == 'https://api.github.com',
            'fixed GitHub API required')
    require(os.environ.get('GH_HOST', 'github.com') == 'github.com', 'fixed GitHub host required')
    result = {'kind': kind, 'repo': repo, 'sha': os.environ.get('GITHUB_SHA', '')}
    require(re.fullmatch('[a-f0-9]{40}', result['sha']) is not None, 'invalid workflow SHA')
    for key, variable in (('run', 'GITHUB_RUN_ID'), ('attempt', 'GITHUB_RUN_ATTEMPT'),
                          ('id', 'ARTIFACT_ID')):
        value = os.environ.get(variable, '')
        require(re.fullmatch('[1-9][0-9]{0,19}', value) is not None, 'invalid ' + variable)
        result[key] = int(value)
    result['digest'] = checksum(os.environ.get('ARTIFACT_DIGEST', ''))
    result['name'] = f"apt-{kind}-{result['run']}-{result['attempt']}"
    return result


def api(route, limit=MAX_JSON):
    token = os.environ.get('GH_TOKEN', '')
    require(bool(token), 'GH_TOKEN required')
    with tempfile.TemporaryDirectory(prefix='velnor-gh-') as config:
        environment = {'PATH': os.environ.get('PATH', ''), 'GH_TOKEN': token,
                       'GH_HOST': 'github.com', 'GH_CONFIG_DIR': config,
                       'GH_PROMPT_DISABLED': '1'}
        command = ['gh', 'api', '--hostname', 'github.com', '--method', 'GET',
                   '-H', 'Accept: application/vnd.github+json',
                   '-H', 'X-GitHub-Api-Version: 2022-11-28', route]
        process = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                                   env=environment)
        timer = threading.Timer(120, process.kill)
        timer.start()
        try:
            payload = process.stdout.read(limit + 1)
            require(len(payload) <= limit, 'GitHub response exceeds size bound')
            require(process.wait(timeout=5) == 0, 'GitHub API request failed')
            return payload
        finally:
            timer.cancel()
            if process.poll() is None:
                process.kill()
            process.wait()
            process.stdout.close()


def document(route):
    return json.loads(api(route).decode('utf-8'), object_pairs_hook=pairs)


def verify_run(ctx, run):
    require(run.get('id') == ctx['run'] and run.get('run_attempt') == ctx['attempt'],
            'workflow run or attempt mismatch')
    require(run.get('head_sha') == ctx['sha'], 'workflow source SHA mismatch')
    repository = run.get('repository', {})
    require(repository.get('full_name') == ctx['repo'], 'workflow repository mismatch')
    require(isinstance(repository.get('id'), int), 'workflow repository ID missing')
    return repository['id']


def verify_producer(ctx):
    route = f"repos/{ctx['repo']}/actions/runs/{ctx['run']}/attempts/{ctx['attempt']}/jobs"
    jobs = []
    for page in range(1, 11):
        response = document(f'{route}?per_page=100&page={page}')
        entries = response.get('jobs')
        require(isinstance(entries, list), 'invalid producer jobs response')
        jobs.extend(entries)
        if len(entries) < 100:
            break
    else:
        raise ValueError('producer jobs exceed pagination bound')
    matches = [job for job in jobs if job.get('name') == JOBS[ctx['kind']]]
    require(len(matches) == 1, 'exact producer job missing or ambiguous')
    job = matches[0]
    require(job.get('run_id') == ctx['run'] and job.get('run_attempt') == ctx['attempt'],
            'producer run or attempt mismatch')
    require(job.get('head_sha') == ctx['sha'], 'producer source SHA mismatch')
    require(job.get('status') == 'completed' and job.get('conclusion') == 'success',
            'producer job must have succeeded')


def verify_artifact(ctx, artifact, repository_id):
    require(artifact.get('id') == ctx['id'] and artifact.get('name') == ctx['name'],
            'artifact ID or name mismatch')
    require(artifact.get('expired') is False, 'artifact expired')
    size = artifact.get('size_in_bytes')
    require(type(size) is int and 0 < size <= MAX_ARCHIVE, 'invalid artifact size')
    require(checksum(artifact.get('digest'), rest=True) == ctx['digest'], 'artifact receipt mismatch')
    run = artifact.get('workflow_run', {})
    require(run.get('id') == ctx['run'], 'artifact workflow run mismatch')
    require(run.get('head_sha') == ctx['sha'], 'artifact source SHA mismatch')
    require(run.get('repository_id') == repository_id and run.get('head_repository_id') == repository_id,
            'artifact repository mismatch')
    if 'run_attempt' in run:
        require(run['run_attempt'] == ctx['attempt'], 'artifact attempt mismatch')
    return size


def members(archive):
    entries = archive.infolist()
    require(0 < len(entries) <= MAX_ENTRIES, 'ZIP entry count exceeds bound')
    seen, files, spelling, total = set(), set(), {}, 0
    for entry in entries:
        name = entry.filename
        require(entry.orig_filename == name, 'unsafe ZIP path')
        require(re.fullmatch('[-A-Za-z0-9._~+/]+', name) is not None, 'unsafe ZIP path')
        parts = name.rstrip('/').split('/')
        require(all(part not in ('', '.', '..') for part in parts), 'unsafe ZIP path')
        require(len(name) <= 512 and len(parts) <= 32, 'ZIP path exceeds bound')
        for index in range(1, len(parts) + 1):
            prefix = '/'.join(parts[:index])
            require(spelling.get(prefix.casefold(), prefix) == prefix, 'case duplicate ZIP path')
            spelling[prefix.casefold()] = prefix
        canonical = '/'.join(parts).casefold()
        require(canonical not in seen, 'duplicate ZIP path')
        require(not any('/'.join(parts[:i]).casefold() in files for i in range(1, len(parts))),
                'ZIP file used as directory')
        require(not any(path.startswith(canonical + '/') for path in seen) or entry.is_dir(),
                'ZIP directory replaced by file')
        seen.add(canonical)
        mode = stat.S_IFMT(entry.external_attr >> 16)
        require(mode in ((0, stat.S_IFDIR) if entry.is_dir() else (0, stat.S_IFREG)),
                'ZIP links and special files forbidden')
        require(not entry.flag_bits & 1, 'encrypted ZIP forbidden')
        require(entry.compress_type in (zipfile.ZIP_STORED, zipfile.ZIP_DEFLATED),
                'unsupported ZIP compression')
        require(entry.file_size <= MAX_FILE, 'ZIP file exceeds bound')
        require(not entry.is_dir() or entry.file_size == 0, 'ZIP directory has payload')
        total += entry.file_size
        require(total <= MAX_CONTENT, 'ZIP expanded size exceeds bound')
        if not entry.is_dir():
            files.add(canonical)
    require(bool(files), 'empty ZIP artifact')
    return entries


def extract(payload, kind):
    destination = Path(ROOTS[kind])
    require(not destination.exists() and not destination.is_symlink(), 'artifact destination exists')
    with zipfile.ZipFile(io.BytesIO(payload)) as archive:
        entries = members(archive)
        with tempfile.TemporaryDirectory(prefix='.velnor-apt-', dir='.') as temporary:
            root = Path(temporary) / 'artifact'
            root.mkdir()
            for entry in entries:
                path = root / entry.filename
                if entry.is_dir():
                    path.mkdir(parents=True, exist_ok=True)
                    continue
                path.parent.mkdir(parents=True, exist_ok=True)
                with archive.open(entry) as source, path.open('xb') as target:
                    shutil.copyfileobj(source, target, 1024 * 1024)
                require(path.stat().st_size == entry.file_size, 'ZIP member size mismatch')
            require(not destination.exists() and not destination.is_symlink(), 'artifact destination exists')
            root.rename(destination)


def download(kind):
    ctx = context(kind)
    base = f"repos/{ctx['repo']}/actions"
    repository_id = verify_run(ctx, document(f"{base}/runs/{ctx['run']}"))
    verify_producer(ctx)
    artifact = document(f"{base}/artifacts/{ctx['id']}")
    expected_size = verify_artifact(ctx, artifact, repository_id)
    payload = api(f"{base}/artifacts/{ctx['id']}/zip", MAX_ARCHIVE)
    require(len(payload) == expected_size, 'artifact archive size mismatch')
    require(hashlib.sha256(payload).hexdigest() == ctx['digest'], 'artifact archive digest mismatch')
    # Recheck the latest attempt after network IO: a rerun invalidates prior outputs.
    verify_run(ctx, document(f"{base}/runs/{ctx['run']}"))
    extract(payload, kind)


if __name__ == '__main__':
    require(len(sys.argv) == 2, 'usage: delivery_apt_transport.py incoming|staging')
    download(sys.argv[1])

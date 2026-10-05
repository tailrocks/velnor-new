import datetime
import hashlib
import importlib.util
import json
import os
import pathlib
import shutil
import stat
import subprocess
import sys
import tempfile
import zlib

TASK = pathlib.Path('/root/.velnor-generator-preview-20261005')
HELPER = TASK / 'preseed-preview-source-materialize.py'
EXPECTED = '27d65c5831d1e9409cb196868624f89aef06b66c6f34b793a49c1f356c876c97'
FIXTURE = TASK / 'source-materializer-fixture-result.json'
assert os.environ == {'PATH': '/usr/bin:/bin', 'LC_ALL': 'C'}
assert sys.flags.isolated and sys.flags.no_site
helper_bytes = HELPER.read_bytes()
helper_sha = hashlib.sha256(helper_bytes).hexdigest()
assert helper_sha == EXPECTED, (helper_sha, EXPECTED)
spec = importlib.util.spec_from_file_location('reviewed_source_materializer', HELPER)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
module.pinned_file('/usr/bin/git', module.GIT_SHA256, module.GIT_SIZE)
scratch = pathlib.Path(tempfile.mkdtemp(prefix='source-materializer-fixture-', dir=TASK))
os.chmod(scratch, 0o700)
repo = scratch / 'repo with spaces ; touch shell-marker'
repo.mkdir(mode=0o700)
git_dir = repo / '.git'
git_dir.mkdir(mode=0o700)
(git_dir / 'objects').mkdir(mode=0o700)
(git_dir / 'refs').mkdir(mode=0o700)
git_config = git_dir / 'config'
git_config.write_text('[core]\n\trepositoryformatversion = 0\n\tfilemode = true\n\tbare = true\n', encoding='ascii')
(git_dir / 'HEAD').write_text('ref: refs/heads/main\n', encoding='ascii')
marker = scratch / 'shell-marker'
module.TASK = str(scratch)
module.SOURCE = str(repo)
payload = b'fixture blob payload\n'
raw = b'blob ' + str(len(payload)).encode('ascii') + b'\0' + payload
oid = hashlib.sha1(raw).hexdigest()
object_dir = git_dir / 'objects' / oid[:2]
object_dir.mkdir(mode=0o700)
object_path = object_dir / oid[2:]
object_path.write_bytes(zlib.compress(raw))
os.chmod(object_path, 0o600)
response = module.git(['cat-file', '--batch'], (oid + '\n').encode('ascii'))
expected = (oid + ' blob ' + str(len(payload)) + '\n').encode('ascii') + payload + b'\n'
assert response == expected, (response, expected)
version = module.git(['--version'])
assert version == module.GIT_VERSION, version
assert not marker.exists(), 'argv was interpreted by a shell'
record = {
    'schema': 1,
    'timestamp_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
    'materializer_sha256': helper_sha,
    'git_sha256': module.GIT_SHA256,
    'git_version': version.decode('ascii').strip(),
    'fixture_repo_path': str(repo),
    'fixture_payload_sha256': hashlib.sha256(payload).hexdigest(),
    'checks': {
        'cat_file_batch_input_delivered': True,
        'no_input_uses_devnull_and_returns_version': True,
        'argv_path_with_spaces_and_semicolon_is_literal': True,
        'shell_marker_absent': True,
    },
    'scope': 'local synthetic Git object only; no upstream source, credentials, network, role, or generator execution',
}
with FIXTURE.open('x', encoding='utf-8') as f:
    json.dump(record, f, sort_keys=True, separators=(',', ':'))
    f.write('\n')
os.chmod(FIXTURE, 0o600)
fixture_sha = hashlib.sha256(FIXTURE.read_bytes()).hexdigest()
print('PASS: stdin, no-input, and literal argv fixture; materializer=' + helper_sha + ' record=' + fixture_sha)
shutil.rmtree(scratch)

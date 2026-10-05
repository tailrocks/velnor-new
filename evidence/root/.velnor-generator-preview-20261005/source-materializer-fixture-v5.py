import datetime
import hashlib
import importlib.util
import json
import os
import pathlib
import shutil
import sys
import tempfile
import stat
import zlib

TASK = pathlib.Path('/root/.velnor-generator-preview-20261005')
HELPER = TASK / 'preseed-preview-source-materialize.py'
SOURCE = TASK / 'main-source-ac3ab'
EXPECTED = 'a325a770c744eb2c6558dd0eee806e791eddc0011ac7c02ff66e46029d0c8fc4'
FIXTURE = TASK / 'source-materializer-fixture-result-v5.json'
assert os.environ == {'PATH': '/usr/bin:/bin', 'LC_ALL': 'C'}
assert sys.flags.isolated and sys.flags.no_site
helper_bytes = HELPER.read_bytes()
helper_sha = hashlib.sha256(helper_bytes).hexdigest()
assert helper_sha == EXPECTED, (helper_sha, EXPECTED)
spec = importlib.util.spec_from_file_location('reviewed_source_materializer', HELPER)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
module.pinned_file('/usr/bin/git', module.GIT_SHA256, module.GIT_SIZE)
module.checked_source(require_empty_worktree=True)
info = SOURCE / '.git' / 'info'
assert info.is_dir() and not info.is_symlink()
assert stat.S_IMODE(info.stat().st_mode) == 0o755
assert module.git(['config', '--local', '--get', 'core.hooksPath']).decode().strip() == '/dev/null'
assert sorted(os.listdir(SOURCE)) == ['.git']
scratch = pathlib.Path(tempfile.mkdtemp(prefix='source-materializer-fixture-v4-', dir=TASK))
os.chmod(scratch, 0o700)
repo = scratch / 'repo with spaces ; touch marker;#'
repo.mkdir(mode=0o700)
git_dir = repo / '.git'
git_dir.mkdir(mode=0o700)
(git_dir / 'objects').mkdir(mode=0o700)
(git_dir / 'refs').mkdir(mode=0o700)
git_config = git_dir / 'config'
git_config.write_text('[core]\n\trepositoryformatversion = 0\n\tfilemode = true\n\tbare = true\n', encoding='ascii')
(git_dir / 'HEAD').write_text('ref: refs/heads/main\n', encoding='ascii')
payload = b'fixture blob payload\n'
raw = b'blob ' + str(len(payload)).encode('ascii') + b'\0' + payload
oid = hashlib.sha1(raw).hexdigest()
object_dir = git_dir / 'objects' / oid[:2]
object_dir.mkdir(mode=0o700)
object_path = object_dir / oid[2:]
object_path.write_bytes(zlib.compress(raw))
os.chmod(object_path, 0o600)
module.TASK = str(scratch)
module.SOURCE = str(repo)
os.chdir(scratch)
response = module.git(['cat-file', '--batch'], (oid + '\n').encode('ascii'))
expected = (oid + ' blob ' + str(len(payload)) + '\n').encode('ascii') + payload + b'\n'
assert response == expected, (response, expected)
version = module.git(['--version'])
assert version == module.GIT_VERSION, version
assert not (scratch / 'marker').exists(), 'argv was interpreted by a shell'
record = {
    'schema': 1,
    'timestamp_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
    'materializer_sha256': helper_sha,
    'git_sha256': module.GIT_SHA256,
    'git_version': version.decode('ascii').strip(),
    'source_commit': module.COMMIT,
    'source_tree': module.TREE,
    'checked_source_created_private_git_info': True,
    'checked_source_disabled_hooks': True,
    'fixture_payload_sha256': hashlib.sha256(payload).hexdigest(),
    'checks': {
        'complete_checked_source_on_authenticated_clone': True,
        'cat_file_batch_input_delivered': True,
        'no_input_uses_devnull_and_returns_version': True,
        'argv_path_with_spaces_semicolon_and_comment_is_literal': True,
        'shell_marker_absent': True,
    },
    'scope': 'exact authenticated no-checkout source guard plus local synthetic Git object; no upstream fetch, credentials, generator, helper execution, role, or container',
}
with FIXTURE.open('x', encoding='utf-8') as f:
    json.dump(record, f, sort_keys=True, separators=(',', ':'))
    f.write('\n')
os.chmod(FIXTURE, 0o600)
fixture_sha = hashlib.sha256(FIXTURE.read_bytes()).hexdigest()
print('PASS: checked_source, stdin, no-input, and literal argv fixture; materializer=' + helper_sha + ' record=' + fixture_sha)
shutil.rmtree(scratch)

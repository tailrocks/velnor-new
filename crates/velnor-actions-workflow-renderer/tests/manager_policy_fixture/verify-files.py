import hashlib, json, os, stat, sys
from pathlib import Path
base = Path('/opt/velnor-manager-fixture')
root = Path(sys.argv[1])
records = json.loads((base/'proof.json').read_text())['mapped_files']
records.update(json.loads((base/'global-policy.json').read_text()))
for name, expected in records.items():
    target = root / name.lstrip('/')
    info = os.lstat(target)
    assert info.st_uid == 0 and stat.S_IMODE(info.st_mode) == expected['mode'], name
    if expected.get('kind') == 'directory':
        assert stat.S_ISDIR(info.st_mode), name
    else:
        assert stat.S_ISREG(info.st_mode), name
        assert hashlib.sha256(target.read_bytes()).hexdigest() == expected['sha256'], name
print('check-only fixture bytes verified; native admission not established')

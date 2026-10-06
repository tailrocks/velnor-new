//! Export only bytes matching immutable, qualified public lock integrities.
//!
//! npm scripts can populate the store with unrelated private packages after ci.
//! Filename checks alone cannot authorize bytes: verify SHA512 before export.
//! Paths follow npm 11.19.0's bundled cacache `lib/content/path.js` and
//! `lib/util/hash-to-segments.js`; this is the native content layout.

pub(super) const SANITIZE: &str = r#"import base64
import hashlib
import os
from pathlib import Path
import stat

def sanitize():
    if os.environ.get('VELNOR_NPM_PUBLIC_PROOF_SAFE') != 'true':
        raise ValueError('public source authority missing')
    allowed = set()
    allowed_dirs = set()
    for integrity in os.environ['VELNOR_NPM_PUBLIC_INTEGRITIES'].splitlines():
        if not integrity.startswith('sha512-'):
            raise ValueError('unqualified integrity')
        digest = base64.b64decode(integrity[7:], validate=True)
        if len(digest) != 64:
            raise ValueError('invalid SHA512')
        value = digest.hex()
        relative = 'sha512/' + value[:2] + '/' + value[2:4] + '/' + value[4:]
        allowed.add(relative)
        allowed_dirs.update(str(parent) for parent in Path(relative).parents if str(parent) != '.')
    root = Path(os.environ['RUNNER_TEMP'])
    if not root.is_absolute() or root.is_symlink() or not root.is_dir():
        raise ValueError('invalid runner temp')
    expected = root / 'velnor' / 'native' / 'npm'
    if os.environ['npm_config_cache'] != str(expected):
        raise ValueError('redirected cache')
    flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW
    owned_fd = os.open(root, flags)
    try:
        for component in ['velnor', 'native', 'npm', '_cacache', 'content-v2']:
            try:
                child_fd = os.open(component, flags, dir_fd=owned_fd)
            except FileNotFoundError:
                return
            os.close(owned_fd)
            owned_fd = child_fd
        for directory, dirs, files, current_fd in os.fwalk('.', dir_fd=owned_fd):
            for name in dirs:
                info = os.stat(name, dir_fd=current_fd, follow_symlinks=False)
                if not stat.S_ISDIR(info.st_mode):
                    raise ValueError('redirected content directory')
            for name in files:
                info = os.stat(name, dir_fd=current_fd, follow_symlinks=False)
                if not stat.S_ISREG(info.st_mode) or info.st_nlink != 1:
                    raise ValueError('redirected content file')
                relative = (Path(directory) / name).as_posix()
                if relative not in allowed:
                    os.unlink(name, dir_fd=current_fd)
                    continue
                digest = hashlib.sha512()
                source_fd = os.open(name, os.O_RDONLY | os.O_NOFOLLOW, dir_fd=current_fd)
                with os.fdopen(source_fd, 'rb') as source:
                    if os.fstat(source.fileno()).st_nlink != 1:
                        raise ValueError('redirected opened content')
                    for chunk in iter(lambda: source.read(1024 * 1024), b''):
                        digest.update(chunk)
                if digest.hexdigest() != relative.replace('/', '')[6:]:
                    os.unlink(name, dir_fd=current_fd)
        for directory, dirs, _, current_fd in os.fwalk('.', topdown=False, dir_fd=owned_fd):
            for name in dirs:
                relative = (Path(directory) / name).as_posix()
                if relative not in allowed_dirs:
                    os.rmdir(name, dir_fd=current_fd)
    finally:
        os.close(owned_fd)

sanitize()
"#;

#[cfg(test)]
mod tests {
    use super::SANITIZE;
    use std::{fs, path::Path, process::Command};

    const PUBLIC: &str = "sha512-LCpQRaCgZUZ2pSPVoon4GB+7kQmwPoFYikmLkF0FXqdAJVV3bjCZahRWiL5jPlsIm5qrqI88s4Zgjwaiu/sTlQ==";
    const RELATIVE: &str = "sha512/2c/2a/5045a0a0654676a523d5a289f8181fbb9109b03e81588a498b905d055ea7402555776e30996a145688be633e5b089b9aaba88f3cb386608f06a2bbfb1395";

    fn command(temp: &Path, integrities: &str) -> Command {
        let mut command = Command::new("python3");
        command.args(["-c", SANITIZE]);
        command.env("RUNNER_TEMP", temp);
        command.env("npm_config_cache", temp.join("velnor/native/npm"));
        command.env("VELNOR_NPM_PUBLIC_INTEGRITIES", integrities);
        command.env("VELNOR_NPM_PUBLIC_PROOF_SAFE", "true");
        command
    }

    #[test]
    fn removes_private_added_content_and_forged_public_bytes() {
        let temp = tempfile::tempdir().expect("temp");
        let store = temp.path().join("velnor/native/npm/_cacache/content-v2");
        let public = store.join(RELATIVE);
        fs::create_dir_all(public.parent().expect("parent")).expect("store");
        fs::write(&public, b"public package bytes").expect("public");
        let private = store.join("sha512/2c/2a/private-token-package");
        fs::write(&private, b"private registry payload").expect("private");
        let private_dirs = store.join("TOKEN_private/arbitrary/nested");
        fs::create_dir_all(&private_dirs).expect("private directory names");
        assert!(
            command(temp.path(), PUBLIC)
                .status()
                .expect("sanitize")
                .success()
        );
        assert_eq!(
            fs::read(&public).expect("public retained"),
            b"public package bytes"
        );
        assert!(!private.exists());
        assert!(!store.join("TOKEN_private").exists());
        fs::write(&public, b"private bytes under public hash").expect("forged");
        assert!(
            command(temp.path(), PUBLIC)
                .status()
                .expect("sanitize")
                .success()
        );
        assert!(!public.exists());
    }

    #[cfg(unix)]
    #[test]
    fn redirected_content_or_ancestors_never_touch_external_files() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().expect("temp");
        let external = tempfile::tempdir().expect("external");
        let secret = external.path().join("secret");
        fs::write(&secret, b"TOKEN").expect("secret");
        let store = temp.path().join("velnor/native/npm/_cacache/content-v2");
        fs::create_dir_all(&store).expect("store");
        symlink(&secret, store.join("private")).expect("symlink");
        assert!(
            !command(temp.path(), PUBLIC)
                .status()
                .expect("sanitize")
                .success()
        );
        assert_eq!(fs::read(&secret).expect("secret intact"), b"TOKEN");
        fs::remove_file(store.join("private")).expect("remove symlink");
        fs::hard_link(&secret, store.join("private")).expect("hardlink");
        assert!(
            !command(temp.path(), PUBLIC)
                .status()
                .expect("sanitize")
                .success()
        );
        assert_eq!(fs::read(&secret).expect("secret intact"), b"TOKEN");
        fs::remove_dir_all(temp.path().join("velnor")).expect("remove owned");
        symlink(external.path(), temp.path().join("velnor")).expect("ancestor");
        assert!(
            !command(temp.path(), PUBLIC)
                .status()
                .expect("sanitize")
                .success()
        );
        assert_eq!(fs::read(&secret).expect("secret intact"), b"TOKEN");
    }

    #[test]
    fn missing_public_proof_cannot_authorize_existing_private_content() {
        let temp = tempfile::tempdir().expect("temp");
        let store = temp.path().join("velnor/native/npm/_cacache/content-v2");
        fs::create_dir_all(&store).expect("store");
        let secret = store.join("private");
        fs::write(&secret, b"private bytes").expect("private");
        let mut missing = command(temp.path(), PUBLIC);
        missing.env_remove("VELNOR_NPM_PUBLIC_PROOF_SAFE");
        assert!(!missing.status().expect("missing proof").success());
        assert_eq!(fs::read(&secret).expect("untouched"), b"private bytes");
        assert!(
            command(temp.path(), "")
                .status()
                .expect("empty proof")
                .success()
        );
        assert!(!secret.exists());
    }
}

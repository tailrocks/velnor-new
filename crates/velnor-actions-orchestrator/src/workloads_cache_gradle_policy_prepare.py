"""Write the fixed policy within the isolated runner cache home."""
import os
from pathlib import Path
import stat


def main():
    runner = Path(os.environ["RUNNER_TEMP"])
    home = Path(os.environ["GRADLE_USER_HOME"])
    if not runner.is_absolute() or runner.resolve() != runner:
        raise ValueError("redirected runner temporary directory")
    if home != runner / "velnor/native/gradle":
        raise ValueError("unqualified Gradle home")
    flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW
    directory = os.open(runner, flags)
    try:
        for name in ("velnor", "native", "gradle"):
            try:
                os.mkdir(name, mode=0o700, dir_fd=directory)
            except FileExistsError:
                pass
            child = os.open(name, flags, dir_fd=directory)
            os.close(directory)
            directory = child
        file_flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW
        target = os.open("velnor-output-policy.init.gradle", file_flags,
                         0o600, dir_fd=directory)
        try:
            info = os.fstat(target)
            if not stat.S_ISREG(info.st_mode) or info.st_nlink != 1:
                raise ValueError("shared policy file")
            data = os.environ["VELNOR_GRADLE_OUTPUT_POLICY"].encode("utf-8")
            while data:
                written = os.write(target, data)
                if written <= 0:
                    raise ValueError("policy write stalled")
                data = data[written:]
        finally:
            os.close(target)
    finally:
        os.close(directory)


if __name__ == "__main__":
    main()

"""Reset unauthenticated Full Mise and Rust state before initialization.

Source3's registry/index, registry/cache and git/db remain separately owned.
This code uses the native owner's nofollow directory/removal primitives.
"""


def rust_cold_prepare(temp, cargo_home, rustup_home, mise_sha):
    if cargo_home != temp + '/velnor/cargo' or rustup_home != temp + '/velnor/rustup':
        raise ValueError('rust_cold_home_binding')
    cold_prepare_optional_manager(temp + '/velnor/mise', mise_sha, temp)
    parent = open_root(temp + '/velnor', temp)
    try:
        for name in ('cargo', 'rustup'):
            try:
                os.mkdir(name, 0o700, dir_fd=parent)
            except FileExistsError:
                pass
        cargo = open_root(cargo_home, temp)
        try:
            # Ambient configuration is outside both executable and Source3
            # payloads. Its presence cannot grant authority to execute Cargo.
            for name in ('config', 'config.toml', 'credentials', 'credentials.toml'):
                try:
                    os.stat(name, dir_fd=cargo, follow_symlinks=False)
                except FileNotFoundError:
                    continue
                raise ValueError('rust_cold_unowned_cargo_configuration')
            for name in ('bin', '.crates.toml', '.crates2.json'):
                try:
                    os.stat(name, dir_fd=cargo, follow_symlinks=False)
                except FileNotFoundError:
                    continue
                remove_entry(cargo, name)
        finally:
            os.close(cargo)
        rustup = open_root(rustup_home, temp)
        try:
            for name in os.listdir(rustup):
                remove_entry(rustup, name)
        finally:
            os.close(rustup)
    finally:
        os.close(parent)

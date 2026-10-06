"""Clear only the fixed root Rust candidate namespace."""


def clear_root_rust_candidate():
    temp = os.environ['RUNNER_TEMP']
    expected = temp + '/' + ROOT_RELATIVE
    if os.environ.get(ROOT_ENV) != expected:
        raise ValueError('root_rust_candidate_root_binding')
    parts = ROOT_RELATIVE.split('/')
    if parts != ['velnor-control', 'root-rust-candidate']:
        raise ValueError('root_rust_candidate_closed_namespace')
    boundary = open_root(temp, temp, allow_boundary=True)
    try:
        try:
            os.mkdir(parts[0], 0o700, dir_fd=boundary)
        except FileExistsError:
            pass
        control = os.open(parts[0], os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW,
                          dir_fd=boundary)
    finally:
        os.close(boundary)
    try:
        info = os.fstat(control)
        if info.st_uid != os.geteuid() or info.st_mode & 0o022:
            raise ValueError('root_rust_candidate_control_owner')
        try:
            remove_entry(control, parts[1])
        except FileNotFoundError:
            pass
        os.mkdir(parts[1], 0o700, dir_fd=control)
    finally:
        os.close(control)

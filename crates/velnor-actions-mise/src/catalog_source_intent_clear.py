"""Fresh purpose genesis; never inspect or execute a restored tool."""


def clear_source_intent():
    temp = os.environ['RUNNER_TEMP']
    expected = temp + '/' + ROOT_RELATIVE
    if os.environ.get(ROOT_ENV) != expected:
        raise ValueError('source_intent_root_binding')
    parts = ROOT_RELATIVE.split('/')
    if parts != ['velnor-control', 'source-intent']:
        raise ValueError('source_intent_closed_namespace')
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
            raise ValueError('source_intent_control_owner')
        try:
            remove_entry(control, parts[1])
        except FileNotFoundError:
            pass
        os.mkdir(parts[1], 0o700, dir_fd=control)
    finally:
        os.close(control)

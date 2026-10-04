"""Before receipt admission, remove selected canonical payload root leaves."""


def clear_selected_roots(temp, roots):
    boundary = open_root(temp, temp, allow_boundary=True)
    try:
        try:
            os.mkdir('velnor', 0o700, dir_fd=boundary)
        except FileExistsError:
            pass
        namespace = os.open('velnor', os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW,
                            dir_fd=boundary)
    finally:
        os.close(boundary)
    try:
        info = os.fstat(namespace)
        if info.st_uid != os.geteuid() or info.st_mode & 0o022:
            raise ValueError('native_clear_namespace_owner')
        for root in roots:
            parts = root.split('/')
            if not root or any(part in ('', '.', '..') for part in parts):
                raise ValueError('native_clear_selected_root')
            descriptor = os.dup(namespace)
            try:
                try:
                    for part in parts[:-1]:
                        child = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW,
                                        dir_fd=descriptor)
                        os.close(descriptor)
                        descriptor = child
                        info = os.fstat(descriptor)
                        if info.st_uid != os.geteuid() or info.st_mode & 0o022:
                            raise ValueError('native_clear_parent_owner')
                    remove_entry(descriptor, parts[-1])
                except FileNotFoundError:
                    pass
            finally:
                os.close(descriptor)
    finally:
        os.close(namespace)

"""Private isolated assembly and rollback of newly installed selected roots."""
import os
import secrets
import stat

from cache_receipt_common import ColdReceipt


class MaterializationStop(Exception):
    """Terminal namespace/rollback failure: cold fallback must never catch this."""


def _remove_at(parent, name, identity=None):
    info = os.stat(name, dir_fd=parent, follow_symlinks=False)
    if identity is not None and (info.st_dev, info.st_ino) != identity:
        raise ColdReceipt("materialize_rollback_identity")
    if stat.S_ISDIR(info.st_mode):
        # This root was created by this transaction; never follow a link.
        os.chmod(name, 0o700, dir_fd=parent, follow_symlinks=False)
        descriptor = os.open(name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=parent)
        try:
            actual = os.fstat(descriptor)
            if (actual.st_dev, actual.st_ino) != (info.st_dev, info.st_ino):
                raise ColdReceipt("materialize_rollback_identity")
            for child in os.listdir(descriptor):
                _remove_at(descriptor, child)
        finally:
            os.close(descriptor)
        os.rmdir(name, dir_fd=parent)
    else:
        os.unlink(name, dir_fd=parent)


class _Assembly:
    def __init__(self, canonical):
        from cache_receipt_materialize import _CanonicalNamespace, _NAMESPACE_AUTHORITY
        self.canonical = canonical
        self.name = ".materialize-" + secrets.token_hex(16)
        self.installed = []
        os.mkdir(self.name, 0o700, dir_fd=canonical._descriptor)
        descriptor = os.open(self.name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW,
                             dir_fd=canonical._descriptor)
        self.identity = (os.fstat(descriptor).st_dev, os.fstat(descriptor).st_ino)
        self.namespace = _CanonicalNamespace(_NAMESPACE_AUTHORITY, descriptor,
            canonical._path + "/" + self.name, canonical._policy, canonical._projection)

    def commit(self, grant):
        from cache_receipt_materialize import _parent_descriptor, _require_empty_roots
        _require_empty_roots(self.canonical, grant.logical_roots)
        for root in grant.logical_roots:
            grant.require_current()
            self.canonical.require(grant)
            try:
                source = _parent_descriptor(self.namespace._descriptor, root)
            except FileNotFoundError:
                if root in grant.optional_roots:
                    continue
                raise ColdReceipt("materialize_assembly_missing") from None
            destination = None
            try:
                name = root.split("/")[-1]
                try:
                    info = os.stat(name, dir_fd=source, follow_symlinks=False)
                except FileNotFoundError:
                    if root in grant.optional_roots:
                        continue
                    raise ColdReceipt("materialize_assembly_missing") from None
                destination = _parent_descriptor(self.canonical._descriptor, root, create=True,
                                                 root_path=self.canonical._path)
                try:
                    os.stat(name, dir_fd=destination, follow_symlinks=False)
                except FileNotFoundError:
                    pass
                else:
                    raise ColdReceipt("materialize_destination_present")
                # Source owner holds both namespaces exclusively throughout.
                identity = info.st_dev, info.st_ino
                retained = os.dup(destination)
                try:
                    os.rename(name, name, src_dir_fd=source, dst_dir_fd=destination)
                except BaseException:
                    os.close(retained)
                    raise
                self.installed.append((retained, name, identity))
                os.fsync(destination)
            finally:
                if destination is not None:
                    os.close(destination)
                os.close(source)

    def rollback(self):
        failures = []
        for parent, name, identity in reversed(self.installed):
            try:
                _remove_at(parent, name, identity)
                os.fsync(parent)
            except (OSError, ColdReceipt) as error:
                failures.append(error)
        if failures:
            raise MaterializationStop("materialize_rollback_failed") from failures[0]

    def close(self):
        self.namespace.close()
        try:
            _remove_at(self.canonical._descriptor, self.name, self.identity)
        except FileNotFoundError:
            pass
        for parent, _, _ in self.installed:
            os.close(parent)
        self.installed.clear()

    def abandon_descriptors(self):
        """Terminal rollback failure: release FDs without touching replaced roots."""
        self.namespace.close()
        for parent, _, _ in self.installed:
            os.close(parent)
        self.installed.clear()

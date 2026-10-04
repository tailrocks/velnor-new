"""Closed, immutable source publication records.

The publisher may select only a record defined here.  A source commit, role,
or protected generator target supplied by a caller never creates authority.
"""

from dataclasses import dataclass
from enum import Enum
from types import MappingProxyType


class SourceRole(str, Enum):
    MISE = "mise"
    MBX_ACTION = "mbx-action"
    SEMVER_CHECKER = "semver-checker"


class ArchiveKind(str, Enum):
    SOURCE_PREFIX_FILES_V1 = "SourcePrefixFilesV1"
    GIT_TAR_UMASK_022_V1 = "GitTarUmask022V1"


@dataclass(frozen=True, slots=True, init=False)
class ReviewedSourceRevision:
    role: SourceRole
    source_commit: str
    source_tree: str
    upstream_base_commit: str
    upstream_repository: str
    archive_kind: ArchiveKind
    source_archive_sha256: str
    source_archive_size: int
    base_patch_sha256: str
    raw_commit_sha256: str
    owner_manifest_sha256: str | None
    source_receipt_sha256: str
    tag_target: str

    def __init__(self, *arguments, **keywords):
        raise TypeError("reviewed source revisions are closed records")

    @property
    def source_ref(self):
        return f"refs/heads/owned-source/{self.role.value}/{self.source_commit}"

    @property
    def tag(self):
        return f"owned-source-{self.role.value}-{self.source_commit}"


_TARGET = "c57c700459bbe1549fe7eedcb7d8689585c38986"


def _record(role, commit, tree, base, upstream, archive_kind, archive_sha,
            archive_size, patch_sha, raw_sha, owner_sha, receipt_sha):
    record = object.__new__(ReviewedSourceRevision)
    values = (SourceRole(role), commit, tree, base, upstream, ArchiveKind(archive_kind),
              archive_sha, archive_size, patch_sha, raw_sha, owner_sha, receipt_sha, _TARGET)
    for field, value in zip(ReviewedSourceRevision.__dataclass_fields__, values):
        object.__setattr__(record, field, value)
    return record


_RECORDS = (
    _record("mise", "dbbf5b0d8f9c7edc5d0111e17ebaf781ecc97a96",
        "d5eefb0470da013d4f524555df763f16a0faf22d", "bc11f90c74eba23bf0d7350efb540e62fb7d9ffd",
        "jdx/mise", "SourcePrefixFilesV1",
        "a2ed2eec09aecf7c964fbf408d6c50fae2ec62a8354f4afb9b5ddaa3812f5d92", 55613440,
        "1faa8dd229d3f403695969fafb41cf5855e68d60fee04c456b851fb95a53637b",
        "fa982e90ef0857b2fb87eb4e4584197b3fc6371664ab47a404e85b7d4c4df773",
        None, "038587e5c20392736ebdfa25d8d10557b774cbca04f13a0c93b4581b83d7e670"),
    _record("mbx-action", "c3cbe8e56ccb4727624df45022357f49d2953075",
        "57a9336f26b9ce4a31f5f914c17594ecaa9c1248", "1687e54eb349cadf61fa38b5813a77875489e8e6",
        "jdx/mr-boxington-action", "SourcePrefixFilesV1",
        "1aa5e5813cdaa08c8240693099c329d261bd93b69e97bef18e773d9e88dadcb1", 1679360,
        "25310d9587c5cf5854014140f0fe97aa800303b21316a660fd67bb77f9c9a554",
        "496ca46524ad9ce2a55b01a454ac4925f897cd09139f08909291a0ac579c1dc0",
        None, "1798003a5f4287d774b6dfa3d54374efd3fce48a8c9543d29306d222458cae8a"),
    _record("mbx-action", "62ec0713473dffeab46884b7c03906042794e696",
        "65205f36cd323748e7256d5deb2694b45eb1664a", "1687e54eb349cadf61fa38b5813a77875489e8e6",
        "jdx/mr-boxington-action", "SourcePrefixFilesV1",
        "90ccfa30f352da933198325470ef986d2feedd05622a0629fb28381d98d124f2", 1689600,
        "32df8091e674fb474780e8ef284bc514bda0ca1c78fd692badbaa3b5bc1ccdb9",
        "467f863fc74c461cd5950fd1df3763f89f52f68847ffca1e9c0d4b6356da1eb5",
        "6ef2008047d98a805cbf494a802457ec3f7b8fae2a864ea9828883600dcb9d60",
        "bf116dc80165075b7c724c6bdca702042620186eb09ba39812d3d18294aab434"),
    _record("semver-checker", "583dddce84706786fc54c41a2c768c28a09c65fd",
        "b0f6ea8b85ac0ed288fc29996e441aaa61bbab48", "4297e8b5f6306531375ba2ba332171e5792b4c38",
        "obi1kenobi/cargo-semver-checks", "GitTarUmask022V1",
        "38573667b13c541e368395259545be0be8f6858ada3b9c158ba93d8633b38c11", 7290880,
        "ae016d81b76419c9d96499c4f527a69867faa776884891d7a391290246708aef",
        "8c4b69df1d8b56c165ed2e9a5a057969e6de58487e8597006bb5889a5f730d36",
        "13139d97abbd474aa0c92c533b81516c0c5e9376dad7d0d88b8a8f8a14b5c71a",
        "e8cfc26a42a979446c669b38ed56cb46de5b267fb399ff8871aa1239e0fae86d"),
)

REVISIONS = MappingProxyType({(record.role.value, record.source_commit): record
                              for record in _RECORDS})


def reviewed_source_revision(role, source_commit, tag_target):
    """Return the exact record or fail before any remote mutation."""
    try:
        role_name = SourceRole(role).value
    except (TypeError, ValueError) as error:
        raise ValueError("unreviewed source role") from error
    record = REVISIONS.get((role_name, source_commit))
    if record is None:
        raise ValueError("unreviewed source revision")
    if tag_target != record.tag_target:
        raise ValueError("source revision protected target differs from reviewed target")
    if not record.source_receipt_sha256 or record.source_receipt_sha256.startswith("__"):
        raise ValueError("source revision receipt is not frozen")
    return record


def role_names():
    return tuple(role.value for role in SourceRole)

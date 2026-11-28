// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

const PATH_LIST: &str = " Double quote \", Colon :, Less than <, Greater than >, Vertical bar |, Asterisk *, Question mark ?, Carriage return \\r, Line feed \\n";
const NAME_LIST: &str = " Double quote \", Colon :, Less than <, Greater than >, Vertical bar |, Asterisk *, Question mark ?, Carriage return \\r, Line feed \\n, Backslash \\, Forward slash /";

pub(super) fn name_error(name: &str, found: &str) -> String {
    format!(
        "::error::The artifact name is not valid: {name}. Contains the following character:  {found}%0A          %0AInvalid characters include: {NAME_LIST}%0A          %0AThese characters are not allowed in the artifact name due to limitations with certain file systems such as NTFS. To maintain file system agnostic behavior, these characters are intentionally not allowed to prevent potential problems with downloads on different file systems."
    )
}

pub(super) fn path_error(path: &str, found: &str) -> String {
    format!(
        "::error::The path for one of the files in artifact is not valid: {path}. Contains the following character:  {found}%0A          %0AInvalid characters include: {PATH_LIST}%0A          %0AThe following characters are not allowed in files that are uploaded due to limitations with certain file systems such as NTFS. To maintain file system agnostic behavior, these characters are intentionally not allowed to prevent potential problems with downloads on different file systems.%0A          "
    )
}

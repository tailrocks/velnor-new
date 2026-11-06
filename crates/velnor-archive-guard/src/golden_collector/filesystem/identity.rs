// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::ffi::OsStrExt;

use sha2::{Digest, Sha256};

use super::{Entry, Tree};

pub(in crate::golden_collector) fn tree_hash(tree: &Tree) -> Vec<u8> {
    let mut output = Vec::new();
    for (path, entry) in tree {
        match entry {
            Entry::File { bytes, executable } => {
                let digest = Sha256::digest(bytes);
                let digest = hex(&digest);
                let mode = if *executable { "x" } else { "-" };
                output.extend_from_slice(
                    format!("{mode} {digest}  ./{}\n", escape_field(path.as_bytes())).as_bytes(),
                );
            }
            Entry::Link(_) | Entry::Directory => {}
        }
    }
    for (path, entry) in tree {
        if let Entry::Link(target) = entry {
            output.extend_from_slice(
                format!(
                    "link ./{} -> {}\n",
                    escape_field(path.as_bytes()),
                    escape_field(target.as_os_str().as_bytes())
                )
                .as_bytes(),
            );
        }
    }
    output
}

pub(in crate::golden_collector) fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    output
}

fn escape_field(bytes: &[u8]) -> String {
    let mut output = String::new();
    for &byte in bytes {
        if (0x20..=0x7e).contains(&byte) && byte != b'\\' && byte != b'>' {
            output.push(char::from(byte));
        } else {
            output.push_str("\\x");
            output.push(char::from(b"0123456789abcdef"[usize::from(byte >> 4)]));
            output.push(char::from(b"0123456789abcdef"[usize::from(byte & 15)]));
        }
    }
    output
}

pub(in crate::golden_collector) fn link_identity(tree: &Tree) -> Vec<u8> {
    tree.iter()
        .filter_map(|(path, entry)| match entry {
            Entry::Link(target) => Some(format!(
                "./{} -> {}\n",
                escape_field(path.as_bytes()),
                escape_field(target.as_os_str().as_bytes())
            )),
            _ => None,
        })
        .collect::<String>()
        .into_bytes()
}

#[cfg(test)]
mod tests {
    use std::os::unix::ffi::OsStringExt;
    use std::path::PathBuf;

    use super::{Entry, Tree, link_identity, tree_hash};

    #[test]
    fn tree_identities_distinguish_ambiguous_and_raw_link_fields() {
        let mut left = Tree::new();
        let mut right = Tree::new();
        left.insert("a -> b".into(), Entry::Link(PathBuf::from("c\nd")));
        right.insert("a".into(), Entry::Link(PathBuf::from("b -> c\nd")));
        assert_ne!(tree_hash(&left), tree_hash(&right));
        assert_ne!(link_identity(&left), link_identity(&right));
        left.clear();
        right.clear();
        left.insert(
            "a\\x0a".into(),
            Entry::Link(PathBuf::from(std::ffi::OsString::from_vec(vec![0x80]))),
        );
        right.insert(
            "a\n".into(),
            Entry::Link(PathBuf::from(std::ffi::OsString::from_vec(vec![0x81]))),
        );
        assert_ne!(tree_hash(&left), tree_hash(&right));
        assert_ne!(link_identity(&left), link_identity(&right));
        right.remove("a\n");
        right.insert(
            "a\\x0a".into(),
            Entry::Link(PathBuf::from(std::ffi::OsString::from_vec(vec![0x81]))),
        );
        assert_ne!(tree_hash(&left), tree_hash(&right));
        assert_ne!(link_identity(&left), link_identity(&right));
    }
}

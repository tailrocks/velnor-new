// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::parse::lines;
use crate::rust_runner::shape::{GUEST_USER, GuestUser};

const RESERVED_IDS: [u32; 2] = [0, 27];

struct Account<'a> {
    name: &'a str,
    uid: u32,
    gid: u32,
}

struct Group<'a> {
    name: &'a str,
    gid: u32,
    members: &'a str,
}

pub(super) struct Databases<'a> {
    pub(super) uid_lookup: &'a str,
    pub(super) all_accounts: &'a str,
    pub(super) gid_lookup: &'a str,
    pub(super) all_groups: &'a str,
}

fn accounts(text: &str) -> Option<Vec<Account<'_>>> {
    lines(text)
        .into_iter()
        .map(|line| {
            let mut fields = line.split(':');
            let name = fields.next()?;
            let _password = fields.next()?;
            let uid = fields.next()?.parse().ok()?;
            let gid = fields.next()?.parse().ok()?;
            Some(Account { name, uid, gid })
        })
        .collect()
}

fn groups(text: &str) -> Option<Vec<Group<'_>>> {
    lines(text)
        .into_iter()
        .map(|line| {
            let mut fields = line.split(':');
            let name = fields.next()?;
            let _password = fields.next()?;
            let gid = fields.next()?.parse().ok()?;
            let members = fields.next()?;
            Some(Group { name, gid, members })
        })
        .collect()
}

pub(super) fn is_sole_identity(user: &GuestUser, found: &Databases<'_>) -> bool {
    let (Some(uid_hits), Some(account_list), Some(gid_hits), Some(group_list)) = (
        accounts(found.uid_lookup),
        accounts(found.all_accounts),
        groups(found.gid_lookup),
        groups(found.all_groups),
    ) else {
        return false;
    };
    let (uid, gid) = (user.uid(), user.gid());
    let own_account = |account: &Account<'_>| account.name == GUEST_USER;
    let own_group = |entry: &Group<'_>| entry.name == GUEST_USER && entry.members.is_empty();
    !RESERVED_IDS.contains(&uid)
        && !RESERVED_IDS.contains(&gid)
        && matches!(uid_hits.as_slice(), [only] if own_account(only) && only.uid == uid)
        && matches!(gid_hits.as_slice(), [only] if own_group(only) && only.gid == gid)
        && matches!(
            account_list.iter().filter(|account| account.uid == uid).collect::<Vec<_>>().as_slice(),
            [only] if own_account(only)
        )
        && matches!(
            account_list.iter().filter(|account| account.gid == gid).collect::<Vec<_>>().as_slice(),
            [only] if own_account(only)
        )
        && matches!(
            group_list.iter().filter(|entry| entry.gid == gid).collect::<Vec<_>>().as_slice(),
            [only] if own_group(only)
        )
        && group_list
            .iter()
            .filter(|entry| entry.name == GUEST_USER)
            .count()
            == 1
}

pub(super) fn sudo_refused(report: &str) -> bool {
    let denial = format!("User {GUEST_USER} is not allowed to run sudo on ");
    matches!(
        lines(report).as_slice(),
        [only] if only.starts_with(&denial) && only.ends_with('.')
    )
}

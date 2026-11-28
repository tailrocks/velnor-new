// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub(crate) const MERGE_GROUP_POLICY: &str = "mode = \"repository\"\nbranches = []\ntag_prefixes = []\nevents = [\"push\", \"merge_group\"]\npull_request_rules = []";

#[derive(Clone, Copy)]
pub(crate) struct Spec<'a> {
    pub(crate) id: &'a str,
    pub(crate) repo: &'a str,
    pub(crate) repository_id: u64,
    pub(crate) scale_set: &'a str,
    pub(crate) credential_ref: &'a str,
    pub(crate) organization: bool,
}

impl<'a> Spec<'a> {
    pub(crate) const fn lab(credential_ref: &'a str) -> Self {
        Self {
            id: "lab",
            repo: "velnor-lab",
            repository_id: 42,
            scale_set: "velnor-mac",
            credential_ref,
            organization: false,
        }
    }

    pub(crate) const fn second() -> Spec<'static> {
        Spec {
            id: "second",
            repo: "velnor-second",
            repository_id: 52,
            scale_set: "velnor-second-set",
            credential_ref: "keychain:com.tailrocks.velnor.host/second",
            organization: false,
        }
    }

    pub(crate) const fn in_organization(mut self) -> Self {
        self.organization = true;
        self
    }

    pub(crate) fn block(&self, merge_group: bool) -> String {
        let (id, repo, rid) = (self.id, self.repo, self.repository_id);
        let organization = if self.organization {
            "organization = \"tailrocks\"\n"
        } else {
            ""
        };
        let policy = if merge_group {
            MERGE_GROUP_POLICY.to_owned()
        } else {
            format!(
                "mode = \"branch_restricted\"\nbranches = [\"main\"]\ntag_prefixes = []\nevents = [\"push\", \"pull_request\"]\npull_request_rules = [{{ base_repository_id = {rid}, base_branch = \"main\", head_repository_id = {rid}, head_branch = \"feature\" }}, {{ base_repository_id = {rid}, base_branch = \"main\", head_repository_id = {next}, head_branch = \"feature\" }}]",
                next = rid + 1,
            )
        };
        format!(
            "[[connections]]\n\
             connection_id = \"{id}\"\n\
             repository_id = {rid}\n\
             repository = \"tailrocks/{repo}\"\n\
             credential_ref = \"{credential_ref}\"\n\
             {organization}\
             [connections.scale_set]\n\
             name = \"{scale_set}\"\n\
             labels = [\"velnor\", \"{scale_set}\"]\n\
             [connections.source_policy]\n\
             {policy}\n",
            credential_ref = self.credential_ref,
            scale_set = self.scale_set,
        )
    }
}

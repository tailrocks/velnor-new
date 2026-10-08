use super::super::VerifiedQueueSession;
use super::SET_ID;
use crate::RepositorySessionCloseClaim;

pub(super) struct TestCloseClaim {
    pub(super) session_id: String,
    pub(super) repository_id: i64,
    pub(super) attempted: bool,
}

impl TestCloseClaim {
    pub(super) fn for_session(session: &VerifiedQueueSession) -> Self {
        Self {
            session_id: session.session_id().to_owned(),
            repository_id: 829_618_808,
            attempted: false,
        }
    }
}

impl RepositorySessionCloseClaim for TestCloseClaim {
    fn intent_id(&self) -> i64 {
        1
    }

    fn destination(&self) -> &'static str {
        "test-destination"
    }

    fn registration_scope(&self) -> &'static str {
        "repository"
    }

    fn scope_name(&self) -> &'static str {
        "ChainArgos/java-monorepo"
    }

    fn target_repository_id(&self) -> i64 {
        self.repository_id
    }

    fn target_repository_full_name(&self) -> &'static str {
        "ChainArgos/java-monorepo"
    }

    fn runner_group_id(&self) -> i64 {
        1
    }

    fn runner_group_name(&self) -> &'static str {
        "Default"
    }

    fn scale_set_id(&self) -> i64 {
        SET_ID
    }

    fn scale_set_name(&self) -> &'static str {
        "synthetic-session-test-set"
    }

    fn session_id_for_cleanup(&self) -> &str {
        &self.session_id
    }

    fn begin_delete_attempt(&mut self) -> bool {
        if self.attempted {
            false
        } else {
            self.attempted = true;
            true
        }
    }
}

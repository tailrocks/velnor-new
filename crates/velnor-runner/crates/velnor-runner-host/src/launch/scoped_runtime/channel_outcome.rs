use super::ScopedChannelOutcome;

impl ScopedChannelOutcome {
    pub(crate) fn request_id(&self) -> Option<i64> {
        let acquired = match self {
            Self::NoStart => return None,
            Self::Ready(ready) => ready.bound().prepared().acquired(),
            Self::PendingPreparation(pending) => pending.prepared().acquired(),
            Self::PendingJit(pending) => pending.bound().prepared().acquired(),
            Self::PendingChannel(pending) => pending.bound().prepared().acquired(),
        };
        acquired.request().request_id()
    }

    pub(crate) fn intent_id(&self) -> Option<i64> {
        let acquired = match self {
            Self::NoStart => return None,
            Self::Ready(ready) => ready.bound().prepared().acquired(),
            Self::PendingPreparation(pending) => pending.prepared().acquired(),
            Self::PendingJit(pending) => pending.bound().prepared().acquired(),
            Self::PendingChannel(pending) => pending.bound().prepared().acquired(),
        };
        Some(acquired.intent_id())
    }
}

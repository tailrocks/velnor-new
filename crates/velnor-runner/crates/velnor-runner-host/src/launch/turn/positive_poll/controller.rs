use super::{
    Acknowledgement, AsyncFnMut, ConnectionJournal, EnsureError, HeaderAllocation, Link, Poll,
    PositiveAdmission, PositivePollOutcome, QueueSession, ack_ready, begin_poll, completion_error,
    intake_and_ack_if_only_scoped, invoke_admission, poll_scoped_queue,
};

pub(crate) async fn positive_poll_once<F>(
    link: &mut Link,
    set_id: i64,
    session: &mut QueueSession,
    scoped: &ConnectionJournal,
    session_intent_id: i64,
    allocation: &HeaderAllocation,
    cursor: i64,
    admit: F,
) -> Result<PositivePollOutcome, EnsureError>
where
    F: AsyncFnMut(
        &mut Link,
        &Poll,
        Option<String>,
        String,
        String,
        String,
    ) -> Result<PositiveAdmission, EnsureError>,
{
    positive_poll_once_core(
        link,
        set_id,
        session,
        scoped,
        session_intent_id,
        allocation,
        cursor,
        async |link, session, scoped, session_intent_id, set_id, header, cursor| {
            poll_scoped_queue(
                link,
                session,
                scoped,
                session_intent_id,
                set_id,
                header,
                cursor,
            )
            .await
        },
        async |link, session, path, queue, polled| ack_ready(link, session, path, queue, polled),
        admit,
    )
    .await
}

#[cfg(test)]
pub(crate) async fn test_positive_poll_once_with_response<F, A>(
    link: &mut Link,
    set_id: i64,
    session: &mut QueueSession,
    scoped: &ConnectionJournal,
    session_intent_id: i64,
    allocation: &HeaderAllocation,
    cursor: i64,
    polled: Poll,
    path: String,
    queue: Option<String>,
    acknowledge: A,
    admit: F,
) -> Result<PositivePollOutcome, EnsureError>
where
    F: AsyncFnMut(
        &mut Link,
        &Poll,
        Option<String>,
        String,
        String,
        String,
    ) -> Result<PositiveAdmission, EnsureError>,
    A: AsyncFnMut(
        &mut Link,
        &QueueSession,
        String,
        Option<String>,
        &Poll,
    ) -> Result<(), EnsureError>,
{
    let mut response = Some((polled, path, queue));
    positive_poll_once_core(
        link,
        set_id,
        session,
        scoped,
        session_intent_id,
        allocation,
        cursor,
        async |_link, _session, _scoped, _session_intent_id, _set_id, _header, _cursor| {
            response.take().ok_or(EnsureError::Uncertain)
        },
        acknowledge,
        admit,
    )
    .await
}

async fn positive_poll_once_core<F, P, A>(
    link: &mut Link,
    set_id: i64,
    session: &mut QueueSession,
    scoped: &ConnectionJournal,
    session_intent_id: i64,
    allocation: &HeaderAllocation,
    cursor: i64,
    poll_response: P,
    mut acknowledge: A,
    admit: F,
) -> Result<PositivePollOutcome, EnsureError>
where
    F: AsyncFnMut(
        &mut Link,
        &Poll,
        Option<String>,
        String,
        String,
        String,
    ) -> Result<PositiveAdmission, EnsureError>,
    P: AsyncFnMut(
        &mut Link,
        &mut QueueSession,
        &ConnectionJournal,
        i64,
        i64,
        u32,
        i64,
    ) -> Result<(Poll, String, Option<String>), EnsureError>,
    A: AsyncFnMut(
        &mut Link,
        &QueueSession,
        String,
        Option<String>,
        &Poll,
    ) -> Result<(), EnsureError>,
{
    let attempt = begin_poll(scoped, session_intent_id, set_id, session, allocation).await?;
    let (attempt, admin_base, polled, path, queue) = poll_once(
        link,
        session,
        scoped,
        session_intent_id,
        set_id,
        attempt,
        cursor,
        poll_response,
    )
    .await?;
    let decision = match intake_and_ack_if_only_scoped(scoped, set_id, &polled, || async {
        acknowledge(link, session, path.clone(), queue.clone(), &polled).await
    })
    .await
    {
        Ok(decision) => decision,
        Err(error) => {
            scoped
                .mark_session_poll_uncertain(session_intent_id, attempt)
                .await
                .map_err(|_| completion_error())?;
            return Err(error);
        }
    };
    let (attempt, admission) = classify_or_admit(
        link,
        set_id,
        session,
        scoped,
        session_intent_id,
        attempt,
        &polled,
        path.clone(),
        queue.clone(),
        admit,
    )
    .await?;
    super::finish::finish_positive_poll_with_ack(
        link,
        session,
        scoped,
        session_intent_id,
        set_id,
        allocation,
        cursor,
        attempt,
        polled,
        path,
        queue,
        admin_base,
        decision,
        admission,
        async |link, session, admin_base, path, queue, polled| {
            if link.set_base(admin_base).is_err() {
                return Acknowledgement::Deferred;
            }
            match acknowledge(link, session, path.to_owned(), queue.clone(), polled).await {
                Ok(()) => Acknowledgement::Acknowledged,
                Err(_) => Acknowledgement::Uncertain,
            }
        },
    )
    .await
}

async fn poll_once<P>(
    link: &mut Link,
    session: &mut QueueSession,
    scoped: &ConnectionJournal,
    session_intent_id: i64,
    set_id: i64,
    attempt: crate::journal::HeaderPollAttempt,
    cursor: i64,
    mut poll_response: P,
) -> Result<
    (
        crate::journal::HeaderPollAttempt,
        String,
        Poll,
        String,
        Option<String>,
    ),
    EnsureError,
>
where
    P: AsyncFnMut(
        &mut Link,
        &mut QueueSession,
        &ConnectionJournal,
        i64,
        i64,
        u32,
        i64,
    ) -> Result<(Poll, String, Option<String>), EnsureError>,
{
    let admin_base = link.base().to_owned();
    let response = poll_response(
        link,
        session,
        scoped,
        session_intent_id,
        set_id,
        attempt.header_value(),
        cursor,
    )
    .await;
    match response {
        Ok((polled, path, queue)) => Ok((attempt, admin_base, polled, path, queue)),
        Err(error) => {
            scoped
                .mark_session_poll_uncertain(session_intent_id, attempt)
                .await
                .map_err(|_| completion_error())?;
            Err(error)
        }
    }
}

async fn classify_or_admit<F>(
    link: &mut Link,
    set_id: i64,
    session: &QueueSession,
    scoped: &ConnectionJournal,
    session_intent_id: i64,
    attempt: crate::journal::HeaderPollAttempt,
    polled: &Poll,
    path: String,
    queue: Option<String>,
    mut admit: F,
) -> Result<(crate::journal::HeaderPollAttempt, Option<PositiveAdmission>), EnsureError>
where
    F: AsyncFnMut(
        &mut Link,
        &Poll,
        Option<String>,
        String,
        String,
        String,
    ) -> Result<PositiveAdmission, EnsureError>,
{
    match super::classify_owned_terminal_available(scoped, set_id, &session.session_id, polled)
        .await
    {
        Ok(Some(receipt)) => Ok((attempt, Some(PositiveAdmission::OwnedTerminal(receipt)))),
        Ok(None) => {
            invoke_admission(
                link,
                polled,
                queue,
                path,
                session,
                scoped,
                session_intent_id,
                attempt,
                &mut admit,
            )
            .await
        }
        Err(_) => {
            scoped
                .mark_session_poll_uncertain(session_intent_id, attempt)
                .await
                .map_err(|_| completion_error())?;
            Err(completion_error())
        }
    }
}

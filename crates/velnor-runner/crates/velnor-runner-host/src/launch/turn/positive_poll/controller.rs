#[cfg(test)]
use super::TestPollResponse;
use super::finish::FinishContext;
use super::{
    Acknowledgement, AsyncFnMut, ConnectionJournal, EnsureError, Link, Poll, PollCapacityMode,
    PositiveAdmission, PositivePollContext, PositivePollOutcome, QueueSession, ack_ready,
    begin_poll, completion_error, invoke_admission, poll_scoped_queue,
};
#[path = "controller/intake.rs"]
mod intake;
use intake::intake_and_ack;

pub(crate) async fn positive_poll_once_with_pressure<F>(
    context: PositivePollContext<'_>,
    sample: Option<crate::launch::FreshPressureSample>,
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
    positive_poll_once_with_mode(context, PollCapacityMode::Pressure(sample), admit).await
}

async fn positive_poll_once_with_mode<F>(
    context: PositivePollContext<'_>,
    capacity_mode: PollCapacityMode,
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
        context,
        capacity_mode,
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
    context: PositivePollContext<'_>,
    response: TestPollResponse,
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
    let mut response = Some(response);
    positive_poll_once_core(
        context,
        PollCapacityMode::Fixed,
        async |_link, _session, _scoped, _session_intent_id, _set_id, _header, _cursor| {
            let response = response.take().ok_or(EnsureError::Uncertain)?;
            Ok((response.polled, response.path, response.queue))
        },
        acknowledge,
        admit,
    )
    .await
}

async fn positive_poll_once_core<F, P, A>(
    mut context: PositivePollContext<'_>,
    capacity_mode: PollCapacityMode,
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
    let attempt = begin_poll(
        context.scoped,
        context.session_intent_id,
        context.set_id,
        context.session,
        context.allocation,
        capacity_mode,
    )
    .await?;
    let (attempt, admin_base, polled, path, queue) =
        poll_once(&mut context, attempt, poll_response).await?;
    let (attempt, decision) = intake_and_ack(
        &mut context,
        attempt,
        &polled,
        path.clone(),
        queue.clone(),
        &mut acknowledge,
    )
    .await?;
    let (attempt, admission) = classify_or_admit(
        &mut context,
        attempt,
        &polled,
        path.clone(),
        queue.clone(),
        admit,
    )
    .await?;
    super::finish::finish_positive_poll_with_ack(
        FinishContext {
            link: context.link,
            session: context.session,
            scoped: context.scoped,
            session_intent_id: context.session_intent_id,
            set_id: context.set_id,
            allocation: context.allocation,
            cursor: context.cursor,
            attempt,
            polled,
            path,
            queue,
            admin_base,
        },
        decision,
        admission,
        async |link, session, admin_base, path, queue, polled| {
            if link.set_base(admin_base).is_err() {
                return Acknowledgement::Deferred;
            }
            match acknowledge(
                link,
                session,
                path.to_owned(),
                queue.map(str::to_owned),
                polled,
            )
            .await
            {
                Ok(()) => Acknowledgement::Acknowledged,
                Err(_) => Acknowledgement::Uncertain,
            }
        },
    )
    .await
}

async fn poll_once<P>(
    context: &mut PositivePollContext<'_>,
    attempt: crate::journal::HeaderPollAttempt,
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
    let admin_base = context.link.base().to_owned();
    let response = poll_response(
        context.link,
        context.session,
        context.scoped,
        context.session_intent_id,
        context.set_id,
        attempt.header_value(),
        context.cursor,
    )
    .await;
    match response {
        Ok((polled, path, queue)) => Ok((attempt, admin_base, polled, path, queue)),
        Err(error) => {
            context
                .scoped
                .mark_session_poll_uncertain(context.session_intent_id, attempt)
                .await
                .map_err(|_| completion_error())?;
            Err(error)
        }
    }
}

async fn classify_or_admit<F>(
    context: &mut PositivePollContext<'_>,
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
    match super::classify_owned_terminal_available(
        context.scoped,
        context.set_id,
        &context.session.session_id,
        polled,
    )
    .await
    {
        Ok(Some(receipt)) => Ok((attempt, Some(PositiveAdmission::OwnedTerminal(receipt)))),
        Ok(None) => {
            invoke_admission(
                super::AdmissionContext {
                    link: context.link,
                    session: context.session,
                    scoped: context.scoped,
                    session_intent_id: context.session_intent_id,
                    attempt,
                },
                polled,
                queue,
                path,
                &mut admit,
            )
            .await
        }
        Err(_) => {
            context
                .scoped
                .mark_session_poll_uncertain(context.session_intent_id, attempt)
                .await
                .map_err(|_| completion_error())?;
            Err(completion_error())
        }
    }
}

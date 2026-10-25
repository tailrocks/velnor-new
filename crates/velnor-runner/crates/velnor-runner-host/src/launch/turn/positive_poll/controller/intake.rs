use super::{AsyncFnMut, EnsureError, Link, Poll, QueueSession, completion_error};
use crate::journal::HeaderPollAttempt;
use crate::launch::turn::{
    completion_intake::{IntakeDecision, intake_and_ack_if_only_scoped},
    positive_poll::PositivePollContext,
};

pub(super) async fn intake_and_ack<A>(
    context: &mut PositivePollContext<'_>,
    attempt: HeaderPollAttempt,
    polled: &Poll,
    path: String,
    queue: Option<String>,
    acknowledge: &mut A,
) -> Result<(HeaderPollAttempt, IntakeDecision), EnsureError>
where
    A: AsyncFnMut(
        &mut Link,
        &QueueSession,
        String,
        Option<String>,
        &Poll,
    ) -> Result<(), EnsureError>,
{
    match intake_and_ack_if_only_scoped(context.scoped, context.set_id, polled, || async {
        acknowledge(
            context.link,
            context.session,
            path.clone(),
            queue.clone(),
            polled,
        )
        .await
    })
    .await
    {
        Ok(decision) => Ok((attempt, decision)),
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

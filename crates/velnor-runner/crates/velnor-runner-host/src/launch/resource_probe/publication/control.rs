use super::{ControlPublicationInput, challenge_request, decision_request};
use crate::{
    error::HostError,
    journal::{
        ExecutionPublicationParent,
        execution_authorization::{
            ExecutionChallengePublicationClaim, ExecutionDecisionDeliveryClaim,
        },
    },
    worker::execution_authorization::{
        ExecutionChallengePublicationReceipt, ExecutionDecisionDeliveryReceipt,
        challenge_publication_receipt, decision_delivery_receipt, inspect_metadata_volume,
    },
};
use uuid::Uuid;

/// Publish the claimed challenge in the measured control directory.
pub(crate) async fn publish_challenge(
    input: ControlPublicationInput<'_>,
    claim: &ExecutionChallengePublicationClaim,
) -> Result<ExecutionChallengePublicationReceipt, HostError> {
    let ControlPublicationInput {
        docker,
        journal,
        selected,
        images,
        release_secret,
        metadata,
        channel,
        guest_boot_id,
        deadline,
    } = input;
    let probe = Box::pin(
        crate::launch::resource_probe::verify_publication_probe_image_until(
            docker,
            journal,
            selected,
            release_secret,
            deadline,
        ),
    )
    .await?;
    let volume = Box::pin(inspect_metadata_volume(
        docker, selected, images, metadata, deadline,
    ))
    .await?;
    let operation_id = Uuid::new_v4().simple().to_string();
    let request = challenge_request(claim, channel, guest_boot_id, operation_id.clone())?;
    let parent = ExecutionPublicationParent::challenge(claim, metadata, guest_boot_id)?;
    let record = Box::pin(super::execute::publish(
        super::execute::PublisherExecution {
            docker,
            journal,
            selected,
            images,
            probe: &probe,
            metadata,
            metadata_volume: &volume,
            deadline,
        },
        request,
        parent,
    ))
    .await?;
    challenge_publication_receipt(
        claim,
        channel,
        metadata,
        record,
        &operation_id,
        guest_boot_id,
        &volume,
    )
}

/// Publish the claimed decision in the measured control directory.
pub(crate) async fn publish_decision(
    input: ControlPublicationInput<'_>,
    claim: &ExecutionDecisionDeliveryClaim,
) -> Result<ExecutionDecisionDeliveryReceipt, HostError> {
    let ControlPublicationInput {
        docker,
        journal,
        selected,
        images,
        release_secret,
        metadata,
        channel,
        guest_boot_id,
        deadline,
    } = input;
    let probe = Box::pin(
        crate::launch::resource_probe::verify_publication_probe_image_until(
            docker,
            journal,
            selected,
            release_secret,
            deadline,
        ),
    )
    .await?;
    let volume = Box::pin(inspect_metadata_volume(
        docker, selected, images, metadata, deadline,
    ))
    .await?;
    let operation_id = Uuid::new_v4().simple().to_string();
    let request = decision_request(claim, channel, guest_boot_id, operation_id.clone())?;
    let parent = ExecutionPublicationParent::decision(claim, metadata, guest_boot_id)?;
    let record = Box::pin(super::execute::publish(
        super::execute::PublisherExecution {
            docker,
            journal,
            selected,
            images,
            probe: &probe,
            metadata,
            metadata_volume: &volume,
            deadline,
        },
        request,
        parent,
    ))
    .await?;
    decision_delivery_receipt(
        claim,
        channel,
        metadata,
        record,
        &operation_id,
        guest_boot_id,
        &volume,
    )
}

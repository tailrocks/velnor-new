//! Bounded measurements of the selected Docker Linux guest.

pub(crate) mod guest;

/// Accept only an explicit Docker 404 as proof that a resource is absent.
pub(crate) fn confirmed_not_found(error: &bollard::errors::Error) -> bool {
    matches!(
        error,
        bollard::errors::Error::DockerResponseServerError {
            status_code: 404,
            ..
        }
    )
}

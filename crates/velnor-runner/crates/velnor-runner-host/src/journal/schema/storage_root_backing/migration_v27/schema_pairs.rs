use crate::error::HostError;

use super::{
    ATTACHMENT_PATHS, LEGACY_ATTACHMENT_PATHS, PATH_INODE_TRIGGER, TRIGGERS, V29_ATTACHMENT_PATHS,
    attachment_paths_with_inode, normalize,
};
use crate::journal::schema::provisioning::{
    BOUND_STORAGE_ATTACHMENTS, V29_BOUND_STORAGE_ATTACHMENTS,
};

pub(super) fn is_current_v31_pair(attachment_sql: Option<&str>, path_sql: Option<&str>) -> bool {
    attachment_sql.is_some_and(|sql| normalize(sql) == normalize(BOUND_STORAGE_ATTACHMENTS))
        && path_sql.is_some_and(|sql| normalize(sql) == normalize(&attachment_paths_with_inode()))
}

pub(super) async fn validate_current_v31_guards(
    connection: &turso::Connection,
) -> Result<(), HostError> {
    for (name, expected) in TRIGGERS
        .iter()
        .filter(|(name, _)| {
            name.starts_with("bound_storage_path_") || *name == "root_backing_bound_guard"
        })
        .chain(std::iter::once(&PATH_INODE_TRIGGER))
    {
        let mut rows = connection
            .query(
                "SELECT sql FROM sqlite_schema WHERE type = 'trigger' AND name = ?1",
                [*name],
            )
            .await
            .map_err(|_| HostError::Journal)?;
        let row = rows
            .next()
            .await
            .map_err(|_| HostError::Journal)?
            .ok_or(HostError::Journal)?;
        let sql = row.get::<String>(0).map_err(|_| HostError::Journal)?;
        if normalize(&sql) != normalize(expected) {
            return Err(HostError::Journal);
        }
    }
    Ok(())
}

pub(super) fn is_v29_pair(attachment_sql: &str, path_sql: Option<&str>) -> bool {
    normalize(attachment_sql) == normalize(V29_BOUND_STORAGE_ATTACHMENTS)
        && path_sql.is_some_and(|sql| normalize(sql) == normalize(V29_ATTACHMENT_PATHS))
}

pub(super) fn is_v29_with_legacy_paths(attachment_sql: &str, path_sql: Option<&str>) -> bool {
    normalize(attachment_sql) == normalize(V29_BOUND_STORAGE_ATTACHMENTS)
        && path_sql.is_some_and(|sql| normalize(sql) == normalize(LEGACY_ATTACHMENT_PATHS))
}

pub(super) fn is_v29_with_current_paths(attachment_sql: &str, path_sql: Option<&str>) -> bool {
    normalize(attachment_sql) == normalize(V29_BOUND_STORAGE_ATTACHMENTS)
        && path_sql.is_some_and(|sql| normalize(sql) == normalize(ATTACHMENT_PATHS))
}

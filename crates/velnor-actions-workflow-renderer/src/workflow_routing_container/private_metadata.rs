use std::path::Path;

use crate::{RenderError, yaml::Yaml};

use super::{invalid, scalar_text};

const PRIVATE_METADATA_MOUNTS: [(&str, &str); 2] = [
    ("/opt/velnor/job-facts", "/opt/velnor/job-facts"),
    ("/.velnor-runtime-identity", "/.velnor-runtime-identity"),
];
const SAFE_OPTIONS: [&str; 22] = [
    "--add-host",
    "--cap-add",
    "--cap-drop",
    "--cpus",
    "--cpuset-cpus",
    "--cpuset-mems",
    "--dns",
    "--entrypoint",
    "--env",
    "--hostname",
    "--init",
    "--label",
    "--memory",
    "--memory-swap",
    "--name",
    "--network",
    "--platform",
    "--security-opt",
    "--shm-size",
    "--ulimit",
    "--user",
    "--workdir",
];

pub(super) fn ensure_private_metadata_mounts(
    value: &mut Yaml,
    path: &str,
    job_id: &str,
) -> Result<(), RenderError> {
    let fields = normalize_container(value, path, job_id)?;
    reject_metadata_mount_options(fields, path, job_id)?;
    let volumes = fields.iter_mut().find(|(key, _)| key == "volumes");
    if let Some((_, value)) = volumes {
        match value {
            Yaml::Seq(volumes) => {
                for volume in volumes.iter() {
                    let value = scalar_text(volume).ok_or_else(|| {
                        invalid(format!("container_option_invalid:{path}:{job_id}:volumes"))
                    })?;
                    if contains_expression(value) {
                        return Err(invalid(format!(
                            "container_volume_unresolved:{path}:{job_id}"
                        )));
                    }
                    reject_reserved_volume_target(value, path, job_id)?;
                }
                for (source, target) in PRIVATE_METADATA_MOUNTS {
                    volumes.push(Yaml::str(format!("{source}:{target}:ro")));
                }
            }
            Yaml::Flow(volumes) => {
                for volume in volumes.iter() {
                    if contains_expression(volume) {
                        return Err(invalid(format!(
                            "container_volume_unresolved:{path}:{job_id}"
                        )));
                    }
                    reject_reserved_volume_target(volume, path, job_id)?;
                }
                for (source, target) in PRIVATE_METADATA_MOUNTS {
                    volumes.push(format!("{source}:{target}:ro"));
                }
            }
            _ => {
                return Err(invalid(format!(
                    "container_option_invalid:{path}:{job_id}:volumes"
                )));
            }
        }
        return Ok(());
    }
    fields.push((
        "volumes".to_owned(),
        Yaml::Seq(
            PRIVATE_METADATA_MOUNTS
                .iter()
                .map(|(source, target)| Yaml::str(format!("{source}:{target}:ro")))
                .collect(),
        ),
    ));
    Ok(())
}

fn normalize_container<'a>(
    value: &'a mut Yaml,
    path: &str,
    job_id: &str,
) -> Result<&'a mut Vec<(String, Yaml)>, RenderError> {
    match value {
        Yaml::Map(fields) => Ok(fields),
        Yaml::AnchoredMap { entries, .. } => Ok(entries),
        Yaml::Str(image) | Yaml::Quoted(image) => {
            let image = image.clone();
            *value = Yaml::Map(vec![("image".to_owned(), Yaml::str(image))]);
            let Yaml::Map(fields) = value else {
                unreachable!();
            };
            Ok(fields)
        }
        Yaml::Annotated {
            value: image,
            comment,
        } => {
            *value = Yaml::Map(vec![(
                "image".to_owned(),
                Yaml::Annotated {
                    value: image.clone(),
                    comment: comment.clone(),
                },
            )]);
            let Yaml::Map(fields) = value else {
                unreachable!();
            };
            Ok(fields)
        }
        Yaml::AnchoredScalar { name, value: image } => {
            let name = name.clone();
            let image = image.clone();
            *value = Yaml::Map(vec![(
                "image".to_owned(),
                Yaml::AnchoredScalar { name, value: image },
            )]);
            let Yaml::Map(entries) = value else {
                unreachable!();
            };
            Ok(entries)
        }
        _ => Err(invalid(format!(
            "job_container_fields_required:{path}:{job_id}"
        ))),
    }
}

fn reject_reserved_volume_target(value: &str, path: &str, job_id: &str) -> Result<(), RenderError> {
    let fields = value.split(':').collect::<Vec<_>>();
    let target = match fields.as_slice() {
        [_, target] | [_, target, _] if Path::new(target).is_absolute() => *target,
        _ => return Err(invalid(format!("container_volume_invalid:{path}:{job_id}"))),
    };
    let target_path = Path::new(target);
    if target_path.components().any(|component| {
        matches!(
            component,
            std::path::Component::ParentDir | std::path::Component::CurDir
        )
    }) || PRIVATE_METADATA_MOUNTS.iter().any(|(_, reserved)| {
        let reserved = Path::new(reserved);
        target_path.starts_with(reserved) || reserved.starts_with(target_path)
    }) {
        return Err(invalid(format!(
            "container_volume_reserved_target:{path}:{job_id}"
        )));
    }
    Ok(())
}

fn reject_metadata_mount_options(
    fields: &[(String, Yaml)],
    path: &str,
    job_id: &str,
) -> Result<(), RenderError> {
    let Some(options) = fields
        .iter()
        .find_map(|(key, value)| (key == "options").then(|| scalar_text(value)))
        .flatten()
    else {
        return Ok(());
    };
    if contains_expression(options) || options.contains(['\'', '"', '\\']) {
        return Err(invalid(format!(
            "container_options_unresolved:{path}:{job_id}"
        )));
    }
    let tokens = options.split_ascii_whitespace().collect::<Vec<_>>();
    let mut index = 0;
    while index < tokens.len() {
        let token = tokens[index];
        let (flag, inline_value) = token
            .split_once('=')
            .map_or((token, None), |(flag, value)| (flag, Some(value)));
        if !SAFE_OPTIONS.contains(&flag) {
            return Err(invalid(format!(
                "container_reserved_mount_option:{path}:{job_id}"
            )));
        }
        let requires_value = !matches!(flag, "--init");
        if requires_value && inline_value.is_none() {
            index += 1;
            if tokens.get(index).is_none_or(|value| value.starts_with('-')) {
                return Err(invalid(format!(
                    "container_options_invalid:{path}:{job_id}"
                )));
            }
        } else if inline_value.is_some_and(str::is_empty) {
            return Err(invalid(format!(
                "container_options_invalid:{path}:{job_id}"
            )));
        }
        index += 1;
    }
    Ok(())
}

fn contains_expression(value: &str) -> bool {
    value.contains("${{") || value.contains("}}")
}

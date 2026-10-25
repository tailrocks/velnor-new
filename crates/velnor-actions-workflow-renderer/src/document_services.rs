use crate::yaml::Yaml;

use crate::document_steps::string_map_yaml;

pub(crate) fn services_yaml(services: &[velnor_actions_contract::ServiceContainer]) -> Yaml {
    let mut sorted: Vec<&velnor_actions_contract::ServiceContainer> = services.iter().collect();
    sorted.sort_by(|left, right| left.id.cmp(&right.id));
    Yaml::Map(
        sorted
            .into_iter()
            .map(|service| {
                (
                    service.id.clone(),
                    Yaml::Map(vec![
                        ("image".to_owned(), Yaml::str(service.image.clone())),
                        ("env".to_owned(), string_map_yaml(&service.env)),
                        (
                            "ports".to_owned(),
                            Yaml::Seq(service.ports.iter().cloned().map(Yaml::str).collect()),
                        ),
                        ("options".to_owned(), Yaml::str(service.options.clone())),
                    ]),
                )
            })
            .collect(),
    )
}

pub(crate) fn push_services(
    entries: &mut Vec<(String, Yaml)>,
    services: &[velnor_actions_contract::ServiceContainer],
) {
    if !services.is_empty() {
        entries.push(("services".to_owned(), services_yaml(services)));
    }
}

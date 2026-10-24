use saphyr::Yaml;

pub(super) fn to_routing_yaml(value: &Yaml<'_>) -> Option<crate::yaml::Yaml> {
    use crate::yaml::Yaml as RoutingYaml;
    match untagged(value) {
        Yaml::Value(_) | Yaml::Representation(..) => {
            if let Some(text) = value.as_str() {
                Some(RoutingYaml::str(text))
            } else if let Some(boolean) = value.as_bool() {
                Some(RoutingYaml::Bool(boolean))
            } else {
                value.as_integer().map(RoutingYaml::Int)
            }
        }
        Yaml::Sequence(values) => values
            .iter()
            .map(to_routing_yaml)
            .collect::<Option<Vec<_>>>()
            .map(RoutingYaml::Seq),
        Yaml::Mapping(entries) => entries
            .iter()
            .map(|(key, value)| Some((scalar_text(key)?.to_owned(), to_routing_yaml(value)?)))
            .collect::<Option<Vec<_>>>()
            .map(RoutingYaml::Map),
        _ => None,
    }
}

pub(super) fn scalar_text<'a>(value: &'a Yaml<'_>) -> Option<&'a str> {
    untagged(value).as_str()
}

pub(super) fn mapping_get<'a, 'input>(
    value: &'a Yaml<'input>,
    key: &str,
) -> Option<&'a Yaml<'input>> {
    untagged(value)
        .as_mapping()?
        .iter()
        .find_map(|(name, value)| (scalar_text(name) == Some(key)).then_some(value))
}

pub(super) fn untagged<'a, 'input>(value: &'a Yaml<'input>) -> &'a Yaml<'input> {
    match value {
        Yaml::Tagged(_, inner) => untagged(inner),
        _ => value,
    }
}

use super::MiseVectors;

pub(super) fn classify_mise_vectors(tokens: &[&str]) -> MiseVectors {
    for (at, token) in tokens.iter().enumerate() {
        if *token != "mise" {
            continue;
        }
        let mut cursor = at + 1;
        while cursor < tokens.len() && tokens[cursor].starts_with('-') {
            cursor += 1;
        }
        let Some(subcommand) = tokens.get(cursor) else {
            return MiseVectors::Unclassifiable("truncated".to_owned());
        };
        match *subcommand {
            "install" => return MiseVectors::Install(specs_until_metachar(&tokens[cursor + 1..])),
            "exec" => {}
            other => {
                return MiseVectors::Unclassifiable(format!("unexpected_subcommand:{other}"));
            }
        }
    }
    MiseVectors::NotInstall
}

fn specs_until_metachar(tail: &[&str]) -> Vec<String> {
    let mut specs = Vec::new();
    for spec in tail {
        if matches!(*spec, "&&" | ";" | "||" | "}" | "{" | "(" | ")") {
            break;
        }
        specs.push((*spec).to_owned());
    }
    specs
}

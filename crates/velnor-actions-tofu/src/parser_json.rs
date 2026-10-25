use crate::modules::{ModuleDecl, source_from_json};
use crate::parser::{BlockModel, MAX_DEPTH, ParseError, Walk};

impl Walk {
    pub(crate) fn json_block(
        &mut self,
        kind: &str,
        value: &serde_json::Value,
        depth: usize,
    ) -> Result<(), ParseError> {
        if depth > MAX_DEPTH {
            return Err(ParseError::TooDeep);
        }
        match kind {
            "module" => {
                let entries = value.as_object().ok_or_else(|| ParseError::Syntax {
                    message: format!("{kind}_must_be_object"),
                })?;
                for (name, body) in entries {
                    self.blocks.push(BlockModel {
                        kind: kind.to_owned(),
                        labels: vec![name.clone()],
                    });
                    self.modules.push(ModuleDecl {
                        name: name.clone(),
                        source: source_from_json(body),
                    });
                    self.json_value(body, depth + 1)?;
                }
            }
            "variable" | "output" => {
                let entries = value.as_object().ok_or_else(|| ParseError::Syntax {
                    message: format!("{kind}_must_be_object"),
                })?;
                for (name, body) in entries {
                    self.blocks.push(BlockModel {
                        kind: kind.to_owned(),
                        labels: vec![name.clone()],
                    });
                    self.json_value(body, depth + 1)?;
                }
            }
            "resource" | "data" => {
                let types = value.as_object().ok_or_else(|| ParseError::Syntax {
                    message: format!("{kind}_must_be_object"),
                })?;
                for (resource_type, names) in types {
                    let entries = names.as_object().ok_or_else(|| ParseError::Syntax {
                        message: format!("{kind}_type_must_be_object"),
                    })?;
                    for (name, body) in entries {
                        self.blocks.push(BlockModel {
                            kind: kind.to_owned(),
                            labels: vec![resource_type.clone(), name.clone()],
                        });
                        self.json_value(body, depth + 1)?;
                    }
                }
            }
            "terraform" => {
                self.blocks.push(BlockModel {
                    kind: kind.to_owned(),
                    labels: Vec::new(),
                });
                self.json_terraform(value, depth)?;
            }
            _ => {
                self.blocks.push(BlockModel {
                    kind: kind.to_owned(),
                    labels: Vec::new(),
                });
                self.json_value(value, depth + 1)?;
            }
        }
        Ok(())
    }

    pub(crate) fn json_terraform(
        &mut self,
        value: &serde_json::Value,
        depth: usize,
    ) -> Result<(), ParseError> {
        match value {
            serde_json::Value::Object(object) => {
                if let Some(serde_json::Value::String(literal)) = object.get("required_version") {
                    self.version(literal);
                }
                self.json_value(value, depth + 1)?;
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    self.json_terraform(item, depth + 1)?;
                }
            }
            _ => {
                self.json_value(value, depth + 1)?;
            }
        }
        Ok(())
    }

    pub(crate) fn json_value(
        &mut self,
        value: &serde_json::Value,
        depth: usize,
    ) -> Result<(), ParseError> {
        if depth > MAX_DEPTH {
            return Err(ParseError::TooDeep);
        }
        self.tick()?;
        match value {
            serde_json::Value::String(literal) => self.literal(literal),
            serde_json::Value::Array(items) => {
                for item in items {
                    self.json_value(item, depth + 1)?;
                }
            }
            serde_json::Value::Object(object) => {
                for (_, member) in object {
                    self.json_value(member, depth + 1)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

use serde::Serialize;
use serde_json::Value;

use crate::error::HostError;

pub(super) fn pair<L: Serialize, R: Serialize>(left: &L, right: &R) -> Result<Vec<u8>, HostError> {
    let left = serde_json::to_value(left).map_err(|_| HostError::Docker)?;
    let right = serde_json::to_value(right).map_err(|_| HostError::Docker)?;
    let mut bytes = Vec::new();
    bytes.push(b'[');
    write_value(&left, &mut bytes)?;
    bytes.push(b',');
    write_value(&right, &mut bytes)?;
    bytes.push(b']');
    Ok(bytes)
}

fn write_value(value: &Value, output: &mut Vec<u8>) -> Result<(), HostError> {
    match value {
        Value::Null => output.extend_from_slice(b"null"),
        Value::Bool(value) => output.extend_from_slice(value.to_string().as_bytes()),
        Value::Number(value) => output.extend_from_slice(value.to_string().as_bytes()),
        Value::String(value) => {
            serde_json::to_writer(output, value).map_err(|_| HostError::Docker)?;
        }
        Value::Array(values) => write_array(values, output)?,
        Value::Object(values) => write_object(values, output)?,
    }
    Ok(())
}

fn write_array(values: &[Value], output: &mut Vec<u8>) -> Result<(), HostError> {
    output.push(b'[');
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            output.push(b',');
        }
        write_value(value, output)?;
    }
    output.push(b']');
    Ok(())
}

fn write_object(
    values: &serde_json::Map<String, Value>,
    output: &mut Vec<u8>,
) -> Result<(), HostError> {
    let mut entries: Vec<_> = values.iter().collect();
    entries.sort_unstable_by_key(|(key, _)| *key);
    output.push(b'{');
    for (index, (key, value)) in entries.into_iter().enumerate() {
        if index > 0 {
            output.push(b',');
        }
        serde_json::to_writer(&mut *output, key).map_err(|_| HostError::Docker)?;
        output.push(b':');
        write_value(value, output)?;
    }
    output.push(b'}');
    Ok(())
}

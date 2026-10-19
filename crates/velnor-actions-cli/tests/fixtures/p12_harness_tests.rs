//! Passing evidence uses one fixture-owned clock, with no dated row overrides.
use std::error::Error;

use serde_json::Value;

fn timestamp_count(value: &Value) -> usize {
    match value {
        Value::Object(object) => {
            usize::from(object.contains_key("checked_at"))
                + object.values().map(timestamp_count).sum::<usize>()
        }
        Value::Array(array) => array.iter().map(timestamp_count).sum(),
        _ => 0,
    }
}

#[test]
fn passing_inventory_rows_inherit_the_single_fixture_timestamp() -> Result<(), Box<dyn Error>> {
    let template: Value = serde_json::from_str(include_str!("p12_inventory.json"))?;
    assert_eq!(template["checked_at"], "{CHECKED}");
    assert_eq!(
        timestamp_count(&template),
        1,
        "rows must inherit the fixture clock"
    );
    let fixture = super::passing("p12-single-clock")?;
    let inventory: Value = serde_json::from_str(&std::fs::read_to_string(
        fixture.dir.join(".velnor/freshness-inventory.json"),
    )?)?;
    assert_eq!(inventory["checked_at"], fixture.checked_at);
    assert_eq!(timestamp_count(&inventory), 1);
    for group in ["tools", "actions"] {
        for row in inventory[group].as_array().ok_or("evidence rows missing")? {
            assert!(
                row.get("checked_at").is_none(),
                "{group} row overrides the clock"
            );
        }
    }
    assert!(inventory["runner"].get("checked_at").is_none());
    super::cleanup(&fixture);
    Ok(())
}

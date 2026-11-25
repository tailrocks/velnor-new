// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::json;
use velnor_ci::image_candidate::canonical::encode;

#[test]
fn encodes_sorted_compact_ascii_json_as_the_golden_record_requires() {
    let value = json!({
        "b": [1, true, null, "\u{e9}"],
        "a": {"z": "\"\\\n\r\t\u{8}\u{c}\u{1}\u{7f}", "\u{43a}\u{43b}\u{44e}\u{447}": "\u{1f600} x"},
        "n": -3,
    });
    let expected = concat!(
        r#"{"a":{"z":"\"\\\n\r\t\b\f~0001~007f","~043a~043b~044e~0447":"~d83d~de00 x"},"#,
        r#""b":[1,true,null,"~00e9"],"n":-3}"#,
        "\n"
    )
    .replace('~', "\\u");
    assert_eq!(String::from_utf8_lossy(&encode(&value)), expected);
}

use super::*;

fn prediction(payload: &str) -> ActionPrediction {
    ActionPrediction {
        action: Digest::blake3(b"action"),
        invocation: Digest::blake3(b"invocation"),
        adapter: "rustc".into(),
        payload: payload.into(),
    }
}

#[test]
fn prediction_payload_requires_native_canonical_json_bytes() {
    for payload in ["{}", "null", "[1,true]", "{\"a\":1,\"b\":2}"] {
        assert!(prediction(payload).validate().is_ok(), "{payload}");
    }
    for payload in [
        "{}\n",
        "{ \"a\":1}",
        "{\"b\":2,\"a\":1}",
        "{\"a\":1,\"a\":1}",
        "1.0",
        "\"\\u0061\"",
    ] {
        let error = prediction(payload).validate().unwrap_err();
        assert!(
            error.to_string().contains("not canonical JSON"),
            "{payload}: {error}"
        );
    }
}

#[test]
fn typed_producer_serialization_reuses_native_payload_codec() {
    #[derive(Serialize)]
    struct ProducerPayload {
        version: u8,
        dependencies: Vec<&'static str>,
    }
    let payload = ProducerPayload {
        version: 1,
        dependencies: vec!["input"],
    };
    let bytes = canonical_json(&payload).unwrap();
    let canonical = String::from_utf8(bytes).unwrap();
    assert!(prediction(&canonical).validate().is_ok());
    assert!(
        prediction(&serde_json::to_string(&payload).unwrap())
            .validate()
            .is_err()
    );
}

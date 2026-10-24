#[tokio::test]
async fn current_schema_rejects_missing_pre_jit_settlement_guard() -> Result<(), String> {
    super::rejects_missing_v32_trigger("pre_jit_cleanup_settlement_insert_guard").await
}

#[tokio::test]
async fn current_schema_rejects_missing_pre_jit_pool_guard() -> Result<(), String> {
    super::rejects_missing_v32_trigger("pre_jit_cleanup_settlement_pool_insert_guard").await
}

#[tokio::test]
async fn current_schema_rejects_missing_pre_jit_probe_guard() -> Result<(), String> {
    super::rejects_missing_v32_trigger("pre_jit_cleanup_probe_insert_guard").await
}

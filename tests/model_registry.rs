use rusuh::providers::model_info::ExtModelInfo;
use rusuh::providers::model_registry::ModelRegistry;

fn make_model(id: &str, provider: &str) -> ExtModelInfo {
    ExtModelInfo {
        id: id.to_string(),
        object: "model".into(),
        created: 0,
        owned_by: provider.to_string(),
        provider_type: provider.to_string(),
        display_name: None,
        name: None,
        version: None,
        description: None,
        input_token_limit: 0,
        output_token_limit: 0,
        supported_generation_methods: vec![],
        context_length: 0,
        max_completion_tokens: 0,
        supported_parameters: vec![],
        thinking: None,
        user_defined: false,
    }
}

#[tokio::test]
async fn register_and_list_models() {
    let reg = ModelRegistry::new();
    let models = vec![
        make_model("gpt-4", "openai"),
        make_model("gpt-3.5", "openai"),
    ];

    reg.register_client("client_0", "openai", models).await;

    let available = reg.get_available_models("openai").await;
    let ids: Vec<&str> = available
        .iter()
        .filter_map(|v| v["id"].as_str())
        .collect();
    assert!(ids.contains(&"gpt-4"));
    assert!(ids.contains(&"gpt-3.5"));
}

#[tokio::test]
async fn get_model_providers_multi() {
    let reg = ModelRegistry::new();
    reg.register_client(
        "c1",
        "antigravity",
        vec![make_model("gemini-2.5-pro", "antigravity")],
    )
    .await;
    reg.register_client(
        "c2",
        "gemini",
        vec![make_model("gemini-2.5-pro", "gemini")],
    )
    .await;

    let providers = reg.get_model_providers("gemini-2.5-pro").await;
    assert!(providers.contains(&"antigravity".to_string()));
    assert!(providers.contains(&"gemini".to_string()));
}

#[tokio::test]
async fn unregister_removes_models() {
    let reg = ModelRegistry::new();
    reg.register_client("c1", "openai", vec![make_model("gpt-4", "openai")])
        .await;

    assert_eq!(reg.get_model_count("gpt-4").await, 1);

    reg.unregister_client("c1").await;

    assert_eq!(reg.get_model_count("gpt-4").await, 0);
    assert!(reg.get_model_providers("gpt-4").await.is_empty());
}

#[tokio::test]
async fn ref_counting_multiple_clients() {
    let reg = ModelRegistry::new();
    reg.register_client("c1", "openai", vec![make_model("gpt-4", "openai")])
        .await;
    reg.register_client("c2", "openai", vec![make_model("gpt-4", "openai")])
        .await;

    assert_eq!(reg.get_model_count("gpt-4").await, 2);

    reg.unregister_client("c1").await;
    assert_eq!(reg.get_model_count("gpt-4").await, 1);

    reg.unregister_client("c2").await;
    assert_eq!(reg.get_model_count("gpt-4").await, 0);
}

#[tokio::test]
async fn quota_exceeded_set_and_clear() {
    let reg = ModelRegistry::new();
    reg.register_client("c1", "openai", vec![make_model("gpt-4", "openai")])
        .await;

    // Model is registered
    assert!(reg.client_supports_model("c1", "gpt-4").await);

    // Set quota exceeded — model still registered, but quota tracked
    reg.set_quota_exceeded("c1", "gpt-4").await;
    // client_supports_model only checks registration, not quota
    assert!(reg.client_supports_model("c1", "gpt-4").await);

    // Clear quota
    reg.clear_quota_exceeded("c1", "gpt-4").await;
    assert!(reg.client_supports_model("c1", "gpt-4").await);
}

#[tokio::test]
async fn suspend_and_resume() {
    let reg = ModelRegistry::new();
    reg.register_client("c1", "openai", vec![make_model("gpt-4", "openai")])
        .await;

    // Suspend — model still registered
    reg.suspend_client_model("c1", "gpt-4", "testing").await;
    assert!(reg.client_supports_model("c1", "gpt-4").await);

    // Resume
    reg.resume_client_model("c1", "gpt-4").await;
    assert!(reg.client_supports_model("c1", "gpt-4").await);
}

#[tokio::test]
async fn empty_registry_returns_empty() {
    let reg = ModelRegistry::new();
    assert!(reg.get_available_models("openai").await.is_empty());
    assert!(reg.get_model_providers("nonexistent").await.is_empty());
    assert_eq!(reg.get_model_count("nonexistent").await, 0);
    assert!(!reg.client_supports_model("x", "y").await);
}

#[tokio::test]
async fn register_empty_models_unregisters() {
    let reg = ModelRegistry::new();
    reg.register_client("c1", "openai", vec![make_model("gpt-4", "openai")])
        .await;
    assert_eq!(reg.get_model_count("gpt-4").await, 1);

    // Re-register with empty models → should unregister
    reg.register_client("c1", "openai", vec![]).await;
    assert_eq!(reg.get_model_count("gpt-4").await, 0);
}

#[tokio::test]
async fn reconcile_updates_models() {
    let reg = ModelRegistry::new();
    reg.register_client(
        "c1",
        "openai",
        vec![make_model("gpt-4", "openai"), make_model("gpt-3.5", "openai")],
    )
    .await;

    assert_eq!(reg.get_model_count("gpt-4").await, 1);
    assert_eq!(reg.get_model_count("gpt-3.5").await, 1);

    // Re-register: remove gpt-3.5, add gpt-4o
    reg.register_client(
        "c1",
        "openai",
        vec![make_model("gpt-4", "openai"), make_model("gpt-4o", "openai")],
    )
    .await;

    assert_eq!(reg.get_model_count("gpt-4").await, 1);
    assert_eq!(reg.get_model_count("gpt-4o").await, 1);
    assert_eq!(reg.get_model_count("gpt-3.5").await, 0);
}

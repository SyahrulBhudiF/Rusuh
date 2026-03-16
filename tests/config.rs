use rusuh::config::Config;

#[test]
fn default_config_values() {
    let cfg = Config::default();
    assert_eq!(cfg.port, 0);
    assert!(cfg.host.is_empty());
    assert!(cfg.api_keys.is_empty());
    assert!(!cfg.debug);
    assert_eq!(cfg.request_retry, 0);
}

#[test]
fn listen_addr_defaults() {
    let cfg = Config::default();
    assert_eq!(cfg.listen_addr(), "0.0.0.0:8317");
}

#[test]
fn listen_addr_custom() {
    let mut cfg = Config::default();
    cfg.host = "127.0.0.1".into();
    cfg.port = 9000;
    assert_eq!(cfg.listen_addr(), "127.0.0.1:9000");
}

#[test]
fn yaml_parse_basic() {
    let yaml = r#"
host: "localhost"
port: 3000
api-keys:
  - "key1"
  - "key2"
debug: true
request-retry: 3
routing:
  strategy: "fill-first"
"#;
    let cfg: Config = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(cfg.host, "localhost");
    assert_eq!(cfg.port, 3000);
    assert_eq!(cfg.api_keys, vec!["key1", "key2"]);
    assert!(cfg.debug);
    assert_eq!(cfg.request_retry, 3);
    assert_eq!(cfg.routing.strategy, "fill-first");
}

#[test]
fn yaml_parse_empty() {
    let cfg: Config = serde_yaml::from_str("{}").unwrap();
    assert_eq!(cfg.port, 0);
    assert!(cfg.api_keys.is_empty());
}

#[test]
fn yaml_parse_oauth_model_alias() {
    let yaml = r#"
oauth-model-alias:
  default:
    - name: "claude-sonnet-4-20250514"
      alias: "sonnet"
    - name: "gpt-4o"
      alias: "gpt4"
"#;
    let cfg: Config = serde_yaml::from_str(yaml).unwrap();
    let aliases = cfg.oauth_model_alias.get("default").unwrap();
    assert_eq!(aliases.len(), 2);
    assert_eq!(aliases[0].name, "claude-sonnet-4-20250514");
    assert_eq!(aliases[0].alias, "sonnet");
}

#[test]
fn yaml_parse_openai_compat() {
    let yaml = r#"
openai-compatibility:
  - name: "openrouter"
    base-url: "https://openrouter.ai/api/v1"
    api-key-entries:
      - api-key: "sk-test"
    models:
      - name: "gpt-4"
        alias: "or-gpt4"
"#;
    let cfg: Config = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(cfg.openai_compat.len(), 1);
    assert_eq!(cfg.openai_compat[0].name, "openrouter");
    assert_eq!(
        cfg.openai_compat[0].base_url,
        "https://openrouter.ai/api/v1"
    );
}

#[test]
fn load_optional_nonexistent_returns_none() {
    let result = Config::load_optional("/tmp/nonexistent_rusuh_config_xyz.yaml").unwrap();
    assert!(result.is_none());
}

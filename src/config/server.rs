use serde::Deserialize;

/// Configuration for the REST API server.
#[derive(Debug, Deserialize, Clone, Default)]
pub struct ServerConfig {
    /// Whether the API server is enabled.
    #[serde(default)]
    pub enabled: bool,

    /// Address and port for the HTTP server to listen on.
    #[serde(default = "default_api_server_listen_address")]
    pub listen_address: String,
}

/// Provides the default value for api_server_listen_address.
fn default_api_server_listen_address() -> String {
    "0.0.0.0:8080".to_string()
}

#[cfg(test)]
mod tests {
    use config::Config;

    use super::*;

    #[test]
    fn test_default_server_config() {
        let yaml = r#""#; // Empty YAML should use defaults
        let config = Config::builder()
            .add_source(config::File::from_str(yaml, config::FileFormat::Yaml))
            .build()
            .unwrap()
            .try_deserialize::<ServerConfig>()
            .unwrap();
        assert_eq!(config.enabled, false);
        assert_eq!(config.listen_address, default_api_server_listen_address());
    }

    #[test]
    fn test_custom_server_config() {
        let yaml = r#"
          enabled: true
          listen_address: "0.0.0.0:3333"
        "#;
        let config = Config::builder()
            .add_source(config::File::from_str(yaml, config::FileFormat::Yaml))
            .build()
            .unwrap()
            .try_deserialize::<ServerConfig>()
            .unwrap();
        assert_eq!(config.enabled, true);
        assert_eq!(config.listen_address, "0.0.0.0:3333");
    }
}

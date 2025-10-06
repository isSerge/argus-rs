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

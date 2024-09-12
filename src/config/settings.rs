use serde::Deserialize;
use std::fs;

#[derive(Debug, Clone, Deserialize)]
pub struct Settings {
    pub settings: IndexerSettings,
    pub websockets: WebSocketSettings,
    pub contract: ContractSettings,
    pub matchers: MatchersSettings,
    pub server: ServerSettings,
}

#[derive(Debug, Clone, Deserialize)]
pub struct IndexerSettings {
    pub active_indexers: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WebSocketSettings {
    pub envio_url: String,
    pub subsquid_url: String,
    pub superchain_username: String,
    pub superchain_pass: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ContractSettings {
    pub contract_id: String,
    pub order_limit: i32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MatchersSettings {
    pub matchers: Vec<String>,
    pub matcher_ws_url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerSettings {
    pub server_port: u16,
}

impl Settings {
    pub fn new() -> Self {
        let config_content = fs::read_to_string("config.toml").expect("Failed to read config file");
        toml::from_str(&config_content).expect("Failed to parse config file")
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self::new()
    }
}

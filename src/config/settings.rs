use serde::Deserialize;
use std::fs;

#[derive(Debug, Deserialize)]
pub struct Settings {
    pub settings: IndexerSettings,
    pub websockets: WebSocketSettings,
    pub contract: ContractSettings,
}

#[derive(Debug, Deserialize)]
pub struct IndexerSettings {
    pub active_indexers: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct WebSocketSettings {
    pub envio_url: String,
    pub subsquid_url: String,
    pub superchain_username: String,
    pub superchain_pass: String,
}

#[derive(Debug, Deserialize)]
pub struct ContractSettings {
    pub contract_id: String,
    pub order_limit: i32,
}

impl Settings {
    pub fn new() -> Self {
        let config_content = fs::read_to_string("config.toml")
            .expect("Failed to read config file");
        toml::from_str(&config_content)
            .expect("Failed to parse config file")
    }
}

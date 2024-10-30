use serde::Deserialize;
use std::fs;

#[derive(Debug, Clone, Deserialize)]
pub struct Settings {
    pub matchers: MatchersSettings,
    pub server: ServerSettings,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MatchersSettings {
    pub matchers: Vec<String>,
    pub matcher_ws_port: u16,
    pub batch_size: usize,
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

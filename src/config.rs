
use serde::Deserialize;
use std::collections::HashMap;

// Get secret config values from config.toml
#[derive(Debug, Deserialize)]
pub struct AppConfig {
    pub camera_urls: HashMap<String, String>,
}
pub fn load_config() -> AppConfig {
    let text = std::fs::read_to_string("config.toml")
        .expect("Missing config.toml (copy config.example.toml to config.toml)");
    toml::from_str(&text).expect("Invalid config.toml")
}
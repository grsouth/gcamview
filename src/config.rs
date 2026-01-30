
use serde::Deserialize;
use std::collections::HashMap;

// Get secret config values from config.toml
#[derive(Debug)]
pub struct AppConfig {
    pub cameras: HashMap<String, CameraConfig>,
    pub mqtt: MqttConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct CameraConfig {
    pub url: String,
    pub label: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct MqttConfig {
    pub host: String,
    pub port: u16,
}

impl Default for MqttConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 1883,
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawConfig {
    #[serde(default)]
    cameras: HashMap<String, CameraConfig>,
    #[serde(default)]
    camera_urls: HashMap<String, String>,
    #[serde(default)]
    mqtt: MqttConfig,
}
pub fn load_config() -> AppConfig {
    let text = std::fs::read_to_string("config.toml")
        .expect("Missing config.toml (copy config.example.toml to config.toml)");
    let raw: RawConfig = toml::from_str(&text).expect("Invalid config.toml");
    let mut cameras = raw.cameras;

    if cameras.is_empty() && !raw.camera_urls.is_empty() {
        cameras = raw
            .camera_urls
            .into_iter()
            .map(|(key, url)| {
                (
                    key,
                    CameraConfig {
                        url,
                        label: None,
                    },
                )
            })
            .collect();
    }

    AppConfig {
        cameras,
        mqtt: raw.mqtt,
    }
}

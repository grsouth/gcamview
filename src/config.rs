
use serde::Deserialize;
use std::collections::HashMap;

// Get secret config values from config.toml
#[derive(Debug)]
pub struct AppConfig {
    pub cameras: HashMap<String, CameraConfig>,
    pub mqtt: MqttConfig,
    pub actions: ActionsConfig,
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

#[derive(Debug, Deserialize, Clone)]
pub struct ActionsConfig {
    #[serde(default)]
    pub wake: WakeConfig,
    #[serde(default)]
    pub tts: TtsConfig,
}

impl Default for ActionsConfig {
    fn default() -> Self {
        Self {
            wake: WakeConfig::default(),
            tts: TtsConfig::default(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct WakeConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub commands: Vec<String>,
}

impl Default for WakeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            commands: vec![
                "gdbus call --session --dest org.gnome.ScreenSaver --object-path /org/gnome/ScreenSaver --method org.gnome.ScreenSaver.SimulateUserActivity".to_string(),
                "gdbus call --session --dest org.gnome.ScreenSaver --object-path /org/gnome/ScreenSaver --method org.gnome.ScreenSaver.SetActive false".to_string(),
                "loginctl unlock-session".to_string(),
            ],
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct TtsConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub command: String,
}

impl Default for TtsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            command: "spd-say {text}".to_string(),
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
    #[serde(default)]
    actions: ActionsConfig,
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
        actions: raw.actions,
    }
}

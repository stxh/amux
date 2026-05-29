use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_font_size")]
    pub font_size: f64,
    #[serde(default)]
    pub window: WindowConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowConfig {
    #[serde(default = "default_window_width")]
    pub width: f64,
    #[serde(default = "default_window_height")]
    pub height: f64,
    #[serde(default)]
    pub position: Option<WindowPosition>,
    #[serde(default)]
    pub maximized: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowPosition {
    pub x: f64,
    pub y: f64,
}

fn default_font_size() -> f64 { 18.0 }
fn default_window_width() -> f64 { 1200.0 }
fn default_window_height() -> f64 { 800.0 }

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            width: default_window_width(),
            height: default_window_height(),
            position: None,
            maximized: false,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            font_size: default_font_size(),
            window: WindowConfig::default(),
        }
    }
}

impl Config {
    pub fn load() -> Self {
        let path = get_config_path();
        if let Ok(content) = std::fs::read_to_string(&path) {
            toml::from_str(&content).unwrap_or_else(|e| {
                eprintln!("Failed to parse config at {:?}: {}", path, e);
                Config::default()
            })
        } else {
            let cfg = Config::default();
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(content) = toml::to_string_pretty(&cfg) {
                let _ = std::fs::write(&path, content);
            }
            cfg
        }
    }

    pub fn path() -> PathBuf {
        get_config_path()
    }
}

fn get_config_path() -> PathBuf {
    if let Some(config_dir) = dirs::config_dir() {
        config_dir.join("amux").join("config.toml")
    } else {
        PathBuf::from("amux.toml")
    }
}

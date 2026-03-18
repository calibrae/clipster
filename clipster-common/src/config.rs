use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_bind")]
    pub bind: String,
    #[serde(default)]
    pub db_path: Option<String>,
    #[serde(default)]
    pub image_dir: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub tls: bool,
    #[serde(default)]
    pub tls_cert: Option<String>,
    #[serde(default)]
    pub tls_key: Option<String>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind: default_bind(),
            db_path: None,
            image_dir: None,
            api_key: None,
            tls: false,
            tls_cert: None,
            tls_key: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientConfig {
    pub server_url: String,
    pub api_key: String,
    #[serde(default = "default_device_name")]
    pub device_name: String,
    #[serde(default)]
    pub insecure: bool,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            server_url: "http://localhost:8743".to_string(),
            api_key: String::new(),
            device_name: default_device_name(),
            insecure: false,
        }
    }
}

fn default_bind() -> String {
    "127.0.0.1:8743".to_string()
}

fn default_device_name() -> String {
    hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| "unknown".to_string())
}

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ServerInfo {
    pub name: String,
    pub url: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppConfig {
    pub game_dir: PathBuf,
    pub selected_server: String,
    pub servers: Vec<ServerInfo>,
    pub lang: String,
    pub full_check: bool,
    #[serde(default = "default_auto_update")]
    pub auto_update: bool,
    #[serde(default = "default_preserve_linux_patches")]
    pub preserve_linux_patches: bool,
    #[serde(default, skip_serializing)]
    pub auto_check: Option<bool>,
    #[serde(default, skip_serializing)]
    pub auto_download: Option<bool>,
}

fn default_auto_update() -> bool {
    true
}

fn default_preserve_linux_patches() -> bool {
    cfg!(not(target_os = "windows"))
}

impl Default for AppConfig {
    fn default() -> Self {
        let default_dir = dirs_default_game_dir();

        let default_servers = vec![
            ServerInfo {
                name: "Сервер 2 (Рекомендуется)".to_string(),
                url: "https://s2.hiddenfromhid.site/updater/".to_string(),
            },
            ServerInfo {
                name: "Сервер 1 (Основной)".to_string(),
                url: "https://hiddenfromhid.site/updater/".to_string(),
            },
            ServerInfo {
                name: "Сервер 4".to_string(),
                url: "https://s4.hiddenfromhid.site/updater/".to_string(),
            },
            ServerInfo {
                name: "Сервер 3".to_string(),
                url: "https://s3.hiddenfromhid.site/updater/".to_string(),
            },
        ];

        Self {
            game_dir: default_dir,
            selected_server: "https://s2.hiddenfromhid.site/updater/".to_string(),
            servers: default_servers,
            lang: "ru".to_string(),
            full_check: false,
            auto_update: true,
            preserve_linux_patches: default_preserve_linux_patches(),
            auto_check: None,
            auto_download: None,
        }
    }
}

fn dirs_default_game_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        let candidates = [
            PathBuf::from("."),
            PathBuf::from(r"C:\Games\EndlessWar"),
            PathBuf::from(r"C:\EndlessWar"),
            PathBuf::from(r"D:\Games\EndlessWar"),
        ];
        for c in candidates {
            if c.join("system").join("EndlessWar.exe").exists() {
                return c;
            }
        }
        if let Ok(cur) = std::env::current_dir() {
            if cur.join("system").join("EndlessWar.exe").exists() {
                return cur;
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        if let Ok(cur) = std::env::current_dir() {
            if cur.join("system").join("EndlessWar.exe").exists() {
                return cur;
            }
        }
        if let Ok(home) = std::env::var("HOME") {
            let p = PathBuf::from(home).join("PortProton/etc/EW");
            if p.exists() {
                return p;
            }
        }
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

impl AppConfig {
    pub fn config_path() -> PathBuf {
        #[cfg(target_os = "windows")]
        {
            if let Ok(appdata) = std::env::var("APPDATA") {
                let p = PathBuf::from(appdata).join("open-ew-launcher");
                let _ = fs::create_dir_all(&p);
                return p.join("launcher.json");
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            if let Ok(home) = std::env::var("HOME") {
                let p = PathBuf::from(home).join(".config/open-ew-launcher");
                let _ = fs::create_dir_all(&p);
                return p.join("launcher.json");
            }
        }
        PathBuf::from("launcher.json")
    }

    pub fn open_config_folder() {
        let p = Self::config_path();
        if let Some(parent) = p.parent() {
            #[cfg(target_os = "windows")]
            {
                let _ = Command::new("explorer").arg(parent).spawn();
            }
            #[cfg(not(target_os = "windows"))]
            {
                let _ = Command::new("xdg-open").arg(parent).spawn();
            }
        }
    }

    pub fn load() -> Self {
        let p = Self::config_path();
        if let Ok(data) = fs::read_to_string(&p) {
            if let Ok(mut cfg) = serde_json::from_str::<AppConfig>(&data) {
                if cfg.servers.is_empty() {
                    cfg.servers = Self::default().servers;
                }
                if let Some(ac) = cfg.auto_check {
                    cfg.auto_update = ac || cfg.auto_download.unwrap_or(false);
                }
                return cfg;
            }
        }
        Self::default()
    }

    pub fn save(&self) {
        let p = Self::config_path();
        if let Ok(data) = serde_json::to_string_pretty(self) {
            let _ = fs::write(p, data);
        }
    }
}

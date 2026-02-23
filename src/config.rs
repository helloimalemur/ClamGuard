use anyhow::Result;
use log::{error, info};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum ScheduleInterval {
    None,
    Daily,
    Weekly,
}

impl Default for ScheduleInterval {
    fn default() -> Self {
        Self::None
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Config {
    pub eject_on_infection: bool,
    pub discord_webhooks: String,
    pub slack_webhooks: String,
    pub freshclam_interval_hours: u32,
    pub scheduled_scan_interval: ScheduleInterval,
    pub scheduled_scan_time: String, // HH:MM
    pub scheduled_scan_day: u32,     // 0-6 (Sun-Sat)
    pub show_uninstall_button: bool,
    pub show_quit_button: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            eject_on_infection: true,
            discord_webhooks: String::new(),
            slack_webhooks: String::new(),
            freshclam_interval_hours: 12,
            scheduled_scan_interval: ScheduleInterval::None,
            scheduled_scan_time: "02:00".to_string(),
            scheduled_scan_day: 0,
            show_uninstall_button: false,
            show_quit_button: false,
        }
    }
}

impl Config {
    pub fn get_path() -> PathBuf {
        if let Ok(dir) = std::env::var("CONFIG_DIR") {
            return PathBuf::from(dir).join("config.json");
        }

        // Check if running as root
        let is_root = std::process::Command::new("id")
            .arg("-u")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "0")
            .unwrap_or(false);

        if is_root {
            PathBuf::from("/Library/Application Support/clamguard/config.json")
        } else {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            PathBuf::from(format!(
                "{}/Library/Application Support/clamguard/config.json",
                home
            ))
        }
    }

    pub fn load() -> Self {
        let path = Self::get_path();
        let mut config = if path.exists() {
            match fs::read_to_string(&path) {
                Ok(content) => match serde_json::from_str::<Config>(&content) {
                    Ok(c) => c,
                    Err(e) => {
                        error!("Failed to parse config file: {}, using defaults", e);
                        Config::default()
                    }
                },
                Err(e) => {
                    error!("Failed to read config file: {}, using defaults", e);
                    Config::default()
                }
            }
        } else {
            let default_config = Config::default();
            if let Err(e) = default_config.save() {
                error!("Failed to save default config: {}", e);
            }
            default_config
        };

        // Override with environment variables
        if let Ok(val) = std::env::var("EJECT_ON_INFECTION") {
            config.eject_on_infection = val.to_lowercase() == "true";
        }
        if let Ok(val) = std::env::var("DISCORD_WEBHOOKS") {
            config.discord_webhooks = val;
        }
        if let Ok(val) = std::env::var("SLACK_WEBHOOKS") {
            config.slack_webhooks = val;
        }
        if let Ok(val) = std::env::var("FRESHCLAM_INTERVAL_HOURS") {
            if let Ok(hours) = val.parse::<u32>() {
                config.freshclam_interval_hours = hours;
            }
        }
        if let Ok(val) = std::env::var("SCHEDULED_SCAN_INTERVAL") {
            match val.to_lowercase().as_str() {
                "none" => config.scheduled_scan_interval = ScheduleInterval::None,
                "daily" => config.scheduled_scan_interval = ScheduleInterval::Daily,
                "weekly" => config.scheduled_scan_interval = ScheduleInterval::Weekly,
                _ => {}
            }
        }
        if let Ok(val) = std::env::var("SCHEDULED_SCAN_TIME") {
            config.scheduled_scan_time = val;
        }
        if let Ok(val) = std::env::var("SCHEDULED_SCAN_DAY") {
            if let Ok(day) = val.parse::<u32>() {
                config.scheduled_scan_day = day;
            }
        }
        if let Ok(val) = std::env::var("SHOW_UNINSTALL_BUTTON") {
            config.show_uninstall_button = val.to_lowercase() == "true";
        }
        if let Ok(val) = std::env::var("SHOW_QUIT_BUTTON") {
            config.show_quit_button = val.to_lowercase() == "true";
        }

        config
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::get_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(self)?;
        fs::write(path, content)?;
        info!("Config saved to {:?}", Self::get_path());
        Ok(())
    }
}

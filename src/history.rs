use anyhow::Result;
use chrono::{DateTime, Duration, Local};
use log::{error, info};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum ScanStatus {
    Clean,
    Infected,
    Failed,
}

impl std::fmt::Display for ScanStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScanStatus::Clean => write!(f, "Clean"),
            ScanStatus::Infected => write!(f, "Infected"),
            ScanStatus::Failed => write!(f, "Failed"),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ScanEntry {
    pub timestamp: DateTime<Local>,
    pub path: String,
    pub status: ScanStatus,
    pub details: String,
    #[serde(default)]
    pub infected_files: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct History {
    pub entries: Vec<ScanEntry>,
}

impl History {
    pub fn get_path() -> PathBuf {
        // Use audit log dir as base for now, consistent with where logs are kept
        PathBuf::from(format!(
            "{}/history.json",
            crate::audit::get_audit_log_dir()
        ))
    }

    pub fn load() -> Self {
        let path = Self::get_path();
        if path.exists() {
            match fs::read_to_string(&path) {
                Ok(content) => match serde_json::from_str::<History>(&content) {
                    Ok(mut h) => {
                        h.prune();
                        h
                    }
                    Err(e) => {
                        error!("Failed to parse history file: {}, starting fresh", e);
                        History::default()
                    }
                },
                Err(e) => {
                    error!("Failed to read history file: {}, starting fresh", e);
                    History::default()
                }
            }
        } else {
            History::default()
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::get_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(self)?;
        fs::write(path, content)?;
        Ok(())
    }

    pub fn add_entry(
        &mut self,
        path: String,
        status: ScanStatus,
        details: String,
        infected_files: Vec<String>,
    ) {
        let entry = ScanEntry {
            timestamp: Local::now(),
            path,
            status,
            details,
            infected_files,
        };
        self.entries.push(entry);
        self.prune();
        if let Err(e) = self.save() {
            error!("Failed to save history: {}", e);
        }
    }

    fn prune(&mut self) {
        let one_year_ago = Local::now() - Duration::days(365);
        let original_count = self.entries.len();
        self.entries.retain(|e| e.timestamp > one_year_ago);
        let pruned_count = original_count - self.entries.len();
        if pruned_count > 0 {
            info!("Pruned {} old history entries", pruned_count);
        }
    }

    pub fn to_csv(&self) -> Result<String> {
        let mut buffer = Vec::new();
        {
            let mut wtr = csv::Writer::from_writer(&mut buffer);
            wtr.write_record(&["Timestamp", "Path", "Status", "Details", "Infected Files"])?;
            for entry in &self.entries {
                wtr.write_record(&[
                    entry.timestamp.to_rfc3339(),
                    entry.path.clone(),
                    entry.status.to_string(),
                    entry.details.clone(),
                    entry.infected_files.join("; "),
                ])?;
            }
            wtr.flush()?;
        }
        Ok(String::from_utf8(buffer)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn test_to_csv() {
        let mut history = History::default();
        history.entries.push(ScanEntry {
            timestamp: Local.with_ymd_and_hms(2023, 1, 1, 12, 0, 0).unwrap(),
            path: "/test/path".to_string(),
            status: ScanStatus::Clean,
            details: "No threats".to_string(),
            infected_files: vec!["file1".to_string(), "file2".to_string()],
        });

        let csv = history.to_csv().unwrap();

        assert!(csv.contains("Timestamp,Path,Status,Details,Infected Files"));
        assert!(csv.contains("/test/path"));
        assert!(csv.contains("Clean"));
        assert!(csv.contains("No threats"));
        assert!(csv.contains("file1; file2"));
        assert!(csv.lines().count() >= 2);
    }
}

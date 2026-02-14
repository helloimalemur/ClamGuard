use chrono::Local;
use log::{error, info};
use std::fs::{self, OpenOptions};
use std::io::Write;

pub fn get_audit_log_dir() -> String {
    if let Ok(dir) = std::env::var("AUDIT_LOG_DIR") {
        return dir;
    }

    // Check if running as root
    let is_root = std::process::Command::new("id")
        .arg("-u")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "0")
        .unwrap_or(false);

    if is_root {
        "/Library/Logs/clamguard".to_string()
    } else {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        format!("{}/Library/Logs/clamguard", home)
    }
}

pub fn get_audit_log_file() -> String {
    format!("{}/audit.log", get_audit_log_dir())
}

pub fn log_event(event_type: &str, message: &str) {
    let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S");
    let formatted_message = format!("[{}] [{}] {}\n", timestamp, event_type, message);

    // Also log to the standard logger
    match event_type {
        "ERROR" | "CRITICAL" => error!("{}", message),
        _ => info!("{}", message),
    }

    let log_dir = get_audit_log_dir();
    let log_file = get_audit_log_file();

    // Ensure directory exists
    if let Err(e) = fs::create_dir_all(&log_dir) {
        eprintln!("Failed to create audit log directory {}: {}", log_dir, e);
        return;
    }

    // Try to write to the audit log file
    let file_result = OpenOptions::new().create(true).append(true).open(&log_file);

    match file_result {
        Ok(mut file) => {
            if let Err(e) = file.write_all(formatted_message.as_bytes()) {
                eprintln!("Failed to write to audit log: {}", e);
            }
        }
        Err(e) => {
            // Fallback to a local file if the preferred path is not writable
            let fallback_file = "audit_fallback.log";
            if let Ok(mut file) = OpenOptions::new()
                .create(true)
                .append(true)
                .open(fallback_file)
            {
                let _ = file.write_all(format!("(FALLBACK) {}", formatted_message).as_bytes());
            }
            eprintln!("Failed to open audit log file {}: {}", log_file, e);
        }
    }
}

pub fn log_scan_start(path: &str) {
    log_event(
        "SCAN_START",
        &format!("Starting scan of mount point: {}", path),
    );
}

pub fn log_scan_complete(path: &str, virus_found: bool, details: &str) {
    let status = if virus_found { "INFECTED" } else { "CLEAN" };
    log_event(
        "SCAN_COMPLETE",
        &format!(
            "Scan of {} finished. Status: {}. Details: {}",
            path, status, details
        ),
    );
}

pub fn log_infection(path: &str, details: &str) {
    log_event(
        "INFECTION_DETECTED",
        &format!("Malware found on {}: {}", path, details),
    );
}

pub fn log_update_start() {
    log_event(
        "UPDATE_START",
        "Starting ClamAV database update (freshclam)",
    );
}

pub fn log_update_complete(success: bool, details: &str) {
    let status = if success { "SUCCESS" } else { "FAILED" };
    log_event(
        "UPDATE_COMPLETE",
        &format!("ClamAV database update {}. Details: {}", status, details),
    );
}

pub fn log_service_start() {
    log_event("SERVICE_START", "ClamGuard service starting");
}

pub fn log_service_stop() {
    log_event("SERVICE_STOP", "ClamGuard service stopping");
}

pub fn log_error(context: &str, error: &str) {
    log_event("ERROR", &format!("{}: {}", context, error));
}

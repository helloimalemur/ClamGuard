#[allow(unused)]
use crate::config::Config;
use crate::guard::AppState;
use crate::utils::{find_clamscan, find_freshclam};
use crate::{audit, notifications, utils};
use anyhow::Result;
use log::{error, info, warn};
use regex::Regex;
#[allow(unused)]
use serde::Serialize;
use std::collections::HashSet;
use std::fs;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::Ordering;

pub fn run_freshclam() -> Result<()> {
    let freshclam_path = find_freshclam();
    audit::log_update_start();
    info!("Running freshclam update using: {}", freshclam_path);

    let mut cmd = Command::new(&freshclam_path);

    // Check if we are running as root
    let is_root = std::process::Command::new("id")
        .arg("-u")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "0")
        .unwrap_or(false);

    if is_root {
        // Ensure freshclam runs as root if the service is running as root,
        // to avoid issues with dropping privileges to a user that may not
        // have permissions to the database or log directories.
        info!("Running as root, adding --user=root flag to freshclam");
        cmd.arg("--user=root");
    }

    if let Some(datadir) = utils::get_clamav_datadir(is_root) {
        info!("Using freshclam datadir: {}", datadir);
        cmd.arg(format!("--datadir={}", datadir));
    }

    if let Ok(tempdir) = std::env::var("FRESHCLAM_TEMPDIR") {
        info!("Using freshclam tempdir: {}", tempdir);
        cmd.env("TMPDIR", tempdir);
    } else if !is_root {
        // Default temp dir for non-root to avoid permission issues in /opt/homebrew/var/lib/clamav
        cmd.env("TMPDIR", "/tmp");
    }

    match cmd.output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let combined = format!("STDOUT: {}\nSTDERR: {}", stdout, stderr);

            if output.status.success() {
                audit::log_update_complete(true, "Database updated successfully");
                info!("freshclam update completed successfully");
            } else {
                let status_code = output.status.code().unwrap_or(-1);
                audit::log_update_complete(
                    false,
                    &format!("Exited with code {}. Output: {}", status_code, combined),
                );
                warn!("freshclam update exited with status: {}", status_code);
            }
        }
        Err(e) => {
            audit::log_update_complete(false, &format!("Failed to execute freshclam: {}", e));
            return Err(e.into());
        }
    }

    Ok(())
}

pub fn run_clamscan(
    app_state: Arc<AppState>,
    target_path: &str,
    eject_on_infection: bool,
) -> Result<(bool, String, Vec<String>)> {
    let clamscan_path = find_clamscan();
    let mut virus_found = false;
    let mut infected_files = Vec::new();
    let re_found = regex::Regex::new(r"^(.*): (.*) FOUND$").unwrap();

    let mut dir_exclusions: HashSet<String> = [
        "\\.Spotlight-V100",
        "\\.fseventsd",
        "\\.Trashes",
        "\\.DocumentRevisions-V100",
        "\\.TemporaryItems",
        "\\$RECYCLE\\.BIN",
        "System Volume Information",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let mut file_exclusions = HashSet::new();

    // Check for .clamignore at the root of target_path
    let clamignore_path = Path::new(target_path).join(".clamignore");
    if clamignore_path.exists() {
        info!("Found .clamignore at {}, adding exclusions...", target_path);
        if let Ok(file) = fs::File::open(&clamignore_path) {
            let reader = BufReader::new(file);
            for line in reader.lines() {
                if let Ok(l) = line {
                    let l = l.trim();
                    if !l.is_empty() && !l.starts_with('#') {
                        info!("Adding exclusion from .clamignore: {}", l);
                        file_exclusions.insert(l.to_string());
                    }
                }
            }
        }
    }

    let mut retry_count = 0;
    const MAX_RETRIES: u32 = 5;

    info!("Using clamscan at: {}", clamscan_path);
    audit::log_scan_start(target_path);

    let log_path_root = audit::get_audit_log_dir();
    let log_file_path = format!("{}/clamav_external_scans.log", log_path_root);
    let _ = fs::create_dir_all(&log_path_root)?;
    let mut log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_file_path)
        .or_else(|e| {
            let fallback = "clamav_external_scans.log";
            warn!(
                "Could not open log file {}: {}. Falling back to {}.",
                log_file_path, e, fallback
            );
            OpenOptions::new().create(true).append(true).open(fallback)
        })
        .map_err(|e| {
            error!("Could not open any log file: {}", e);
            e
        })?;

    loop {
        let mut buffer = Vec::new();
        if retry_count == 0 {
            writeln!(
                buffer,
                "\n--- Scan starting for: {} at {} ---",
                target_path,
                chrono::Local::now()
            )?;
        } else {
            writeln!(
                buffer,
                "\n--- Retry #{} for: {} at {} ---",
                retry_count,
                target_path,
                chrono::Local::now()
            )?;
            writeln!(
                buffer,
                "Excluding {} files and {} directories (plus defaults)",
                file_exclusions.len(),
                dir_exclusions.len() - 7
            )?;
        }

        let mut cmd = Command::new(&clamscan_path);
        cmd.arg("-r").arg("--bell");

        let is_root = std::process::Command::new("id")
            .arg("-u")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "0")
            .unwrap_or(false);

        if let Some(datadir) = utils::get_clamav_datadir(is_root) {
            cmd.arg(format!("--database={}", datadir));
        }

        for de in &dir_exclusions {
            cmd.arg(format!("--exclude-dir={}", de));
        }
        for fe in &file_exclusions {
            cmd.arg(format!("--exclude={}", fe));
        }
        cmd.arg(target_path);

        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let mut child = cmd.spawn()?;
        let stdout = child.stdout.take().expect("Failed to open stdout");
        let stderr = child.stderr.take().expect("Failed to open stderr");

        let (tx, rx) = std::sync::mpsc::channel();
        let tx_out = tx.clone();
        let target_path_clone = target_path.to_string();
        let app_state_progress = Arc::clone(&app_state);
        std::thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                if let Ok(l) = line {
                    if l.starts_with('/') {
                        // This looks like a file path being scanned
                        if let Some(colon_idx) = l.find(':') {
                            let file_path = &l[..colon_idx];
                            app_state_progress
                                .update_scan_status(&target_path_clone, file_path.to_string());
                        }
                    }
                    let _ = tx_out.send(l);
                }
            }
        });

        let tx_err = tx.clone();
        std::thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines() {
                if let Ok(l) = line {
                    let _ = tx_err.send(l);
                }
            }
        });

        drop(tx);

        let mut current_stdout = String::new();
        let mut summary = String::new();
        let mut in_summary = false;
        let mut killed = false;

        for line in rx {
            // Check for cancellation
            if !killed {
                if let Ok(scans) = app_state.active_scans.lock() {
                    if let Some(state) = scans.get(target_path) {
                        if state.cancel_flag.load(Ordering::SeqCst) {
                            info!("Scan cancellation requested for {}", target_path);
                            let _ = child.kill();
                            killed = true;
                        }
                    }
                }
            }
            if killed {
                break;
            }

            if let Some(caps) = re_found.captures(&line) {
                let file_path = caps.get(1).unwrap().as_str().to_string();
                if !infected_files.contains(&file_path) {
                    infected_files.push(file_path);
                }
            }

            info!("clamscan [{}]: {}", target_path, line);
            writeln!(buffer, "{}", line)?;
            current_stdout.push_str(&line);
            current_stdout.push('\n');

            if line.contains("----------- SCAN SUMMARY -----------") {
                in_summary = true;
            }
            if in_summary {
                summary.push_str(&line);
                summary.push('\n');
            }
        }

        let status = child.wait()?;

        if killed {
            audit::log_scan_complete(target_path, false, "Scan cancelled by user");
            return Ok((false, "Scan cancelled by user".to_string(), Vec::new()));
        }

        if status.code() == Some(2) && retry_count < MAX_RETRIES {
            let mut found_new_exclusions = false;

            // Regexes for permission-related errors
            let re_access_denied = Regex::new(r"^(.*): Access denied$").unwrap();
            let re_cant_open_dir = Regex::new(r"^(.*): Can't open directory\.$").unwrap();
            let re_libclamav = Regex::new(
                r"LibClamAV Error: cl_scandir: can't open directory (.*) \(Permission denied\)",
            )
            .unwrap();

            for line in current_stdout.lines() {
                let line = line.trim();
                if let Some(caps) = re_access_denied.captures(line) {
                    let path = caps.get(1).unwrap().as_str().trim();
                    if file_exclusions.insert(format!("^{}$", regex::escape(path))) {
                        found_new_exclusions = true;
                    }
                } else if let Some(caps) = re_cant_open_dir.captures(line) {
                    let path = caps.get(1).unwrap().as_str().trim();
                    if dir_exclusions.insert(format!("^{}$", regex::escape(path))) {
                        found_new_exclusions = true;
                    }
                } else if let Some(caps) = re_libclamav.captures(line) {
                    let path = caps.get(1).unwrap().as_str().trim();
                    if dir_exclusions.insert(format!("^{}$", regex::escape(path))) {
                        found_new_exclusions = true;
                    }
                }
            }

            if found_new_exclusions {
                retry_count += 1;
                writeln!(
                    buffer,
                    "Detected permission issues, retrying with additional exclusions..."
                )?;
                log_file.write_all(&buffer)?;
                continue;
            }
        }

        writeln!(
            buffer,
            "--- Scan finished for: {} at {} with exit code: {} ---",
            target_path,
            chrono::Local::now(),
            status
        )?;
        log_file.write_all(&buffer)?;

        if !status.success() {
            // Clamscan exit codes: 0 = no virus, 1 = virus found, 2 = some error occurred
            if status.code() == Some(1) {
                warn!("VIRUS DETECTED on {}", target_path);

                let re_infected = Regex::new(r"Infected files: (\d+)").unwrap();
                let mut infected_count = 0;
                if let Some(caps) = re_infected.captures(&summary) {
                    if let Some(m) = caps.get(1) {
                        infected_count = m.as_str().parse::<u32>().unwrap_or(0);
                    }
                }

                if infected_count > 0 {
                    virus_found = true;
                    audit::log_infection(target_path, &summary.replace("\n", " "));
                    notifications::send_notifications(&summary, target_path);
                    if eject_on_infection {
                        utils::eject_drive(target_path);
                    }
                } else {
                    audit::log_scan_complete(
                        target_path,
                        false,
                        "Scan finished with exit code 1 but no infected files found in summary",
                    );
                }
            } else if status.code() == Some(2) {
                let msg = format!(
                    "clamscan finished with some errors (exit code 2) after {} retries. Check clamav_external_scans.log for details.",
                    retry_count
                );
                error!("{}", msg);
                audit::log_scan_complete(target_path, false, &msg);
                return Ok((false, msg, infected_files));
            } else {
                let msg = format!("clamscan exited with non-zero status: {:?}", status.code());
                error!("{}", msg);
                audit::log_scan_complete(target_path, false, &msg);
                return Ok((false, msg, infected_files));
            }
        } else {
            audit::log_scan_complete(target_path, false, "No threats found");
        }
        return Ok((virus_found, summary, infected_files));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_parsing() {
        let stdout_str = "test_dir/d1/f1: Access denied\ntest_dir/d2: Can't open directory.\n";
        let stderr_str =
            "LibClamAV Error: cl_scandir: can't open directory test_dir/d3 (Permission denied)\n";

        let mut file_exclusions = HashSet::new();
        let mut dir_exclusions = HashSet::new();

        let re_access_denied = Regex::new(r"^(.*): Access denied$").unwrap();
        let re_cant_open_dir = Regex::new(r"^(.*): Can't open directory\.$").unwrap();
        let re_libclamav = Regex::new(
            r"LibClamAV Error: cl_scandir: can't open directory (.*) \(Permission denied\)",
        )
        .unwrap();

        for line in stdout_str.lines().chain(stderr_str.lines()) {
            let line = line.trim();
            if let Some(caps) = re_access_denied.captures(line) {
                let path = caps.get(1).unwrap().as_str().trim();
                file_exclusions.insert(format!("^{}$", regex::escape(path)));
            } else if let Some(caps) = re_cant_open_dir.captures(line) {
                let path = caps.get(1).unwrap().as_str().trim();
                dir_exclusions.insert(format!("^{}$", regex::escape(path)));
            } else if let Some(caps) = re_libclamav.captures(line) {
                let path = caps.get(1).unwrap().as_str().trim();
                dir_exclusions.insert(format!("^{}$", regex::escape(path)));
            }
        }

        assert!(file_exclusions.contains("^test_dir/d1/f1$"));
        assert!(dir_exclusions.contains("^test_dir/d2$"));
        assert!(dir_exclusions.contains("^test_dir/d3$"));
    }

    #[test]
    fn test_clamignore_parsing() {
        let test_ignore = ".test_clamignore";
        fs::write(test_ignore, "# comment\nexclusion1\n\n  exclusion2  \n").unwrap();

        let mut file_exclusions = HashSet::new();
        if let Ok(file) = fs::File::open(test_ignore) {
            let reader = BufReader::new(file);
            for line in reader.lines() {
                if let Ok(l) = line {
                    let l = l.trim();
                    if !l.is_empty() && !l.starts_with('#') {
                        file_exclusions.insert(l.to_string());
                    }
                }
            }
        }

        fs::remove_file(test_ignore).unwrap();

        assert!(file_exclusions.contains("exclusion1"));
        assert!(file_exclusions.contains("exclusion2"));
        assert_eq!(file_exclusions.len(), 2);
    }

    #[test]
    fn test_summary_parsing() {
        let summary = "----------- SCAN SUMMARY -----------\nKnown viruses: 8684783\nEngine version: 1.1.0\nScanned directories: 1\nScanned files: 1\nInfected files: 2\nData scanned: 0.00 MB\n";

        let re_infected = Regex::new(r"Infected files: (\d+)").unwrap();
        let mut infected_count = 0;
        if let Some(caps) = re_infected.captures(summary) {
            if let Some(m) = caps.get(1) {
                infected_count = m.as_str().parse::<u32>().unwrap_or(0);
            }
        }

        assert_eq!(infected_count, 2);
    }
}

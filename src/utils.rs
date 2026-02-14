use anyhow::Result;
use log::{error, info};
use std::fs;
use std::path::Path;
use std::process::Command;
use tray_icon::Icon;

pub fn find_clamscan() -> String {
    let common_paths = [
        "/opt/homebrew/bin/clamscan",
        "/usr/local/bin/clamscan",
        "/usr/bin/clamscan",
    ];
    for path in common_paths {
        if Path::new(path).exists() {
            return path.to_string();
        }
    }
    "clamscan".to_string() // Fallback to PATH
}

pub fn find_freshclam() -> String {
    let common_paths = [
        "/opt/homebrew/bin/freshclam",
        "/usr/local/bin/freshclam",
        "/usr/bin/freshclam",
    ];
    for path in common_paths {
        if Path::new(path).exists() {
            return path.to_string();
        }
    }
    "freshclam".to_string() // Fallback to PATH
}

#[derive(Debug)]
pub enum IconState {
    Idle,
    Active,
    Infected,
}

pub fn create_icon(state: IconState) -> Icon {
    let size = 22;
    let mut rgba = Vec::with_capacity(size * size * 4);
    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - (size as f32 / 2.0);
            let dy = y as f32 - (size as f32 / 2.0);
            let dist = (dx * dx + dy * dy).sqrt();

            // Draw a circle
            if dist < (size as f32 / 2.0) - 4.0 {
                match state {
                    IconState::Active => {
                        // Blue-ish for active
                        rgba.extend_from_slice(&[0, 122, 255, 255]);
                    }
                    IconState::Infected => {
                        // Red-ish for infected
                        rgba.extend_from_slice(&[255, 59, 48, 255]);
                    }
                    IconState::Idle => {
                        // Gray-ish for idle
                        rgba.extend_from_slice(&[128, 128, 128, 255]);
                    }
                }
            } else if dist < (size as f32 / 2.0) - 2.0 {
                // Border
                rgba.extend_from_slice(&[200, 200, 200, 255]);
            } else {
                // Transparent background
                rgba.extend_from_slice(&[0, 0, 0, 0]);
            }
        }
    }
    Icon::from_rgba(rgba, size as u32, size as u32).expect("Failed to create icon")
}

pub fn is_service_installed() -> bool {
    if Path::new("/Library/LaunchDaemons/com.clamguard.plist").exists() {
        return true;
    }
    if let Ok(home) = std::env::var("HOME") {
        if Path::new(&home)
            .join("Library/LaunchAgents/com.clamguard.plist")
            .exists()
        {
            return true;
        }
    }
    false
}

pub fn install_as_service() -> Result<()> {
    // 1. Get current executable path
    let current_exe = std::env::current_exe()?;
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());

    // Use user-local paths to avoid root/system installation
    let support_dir = format!("{}/Library/Application Support/clamguard", home);
    let target_bin = format!("{}/clamguard", support_dir);
    let plist_dir = format!("{}/Library/LaunchAgents", home);
    let plist_path = format!("{}/com.clamguard.plist", plist_dir);
    let log_dir = format!("{}/Library/Logs/clamguard", home);

    info!("Installing service to {}...", target_bin);

    // 2. Create directories
    fs::create_dir_all(&support_dir)?;
    fs::create_dir_all(&log_dir)?;
    fs::create_dir_all(&plist_dir)?;

    // 3. Copy binary
    fs::copy(&current_exe, &target_bin)?;

    // 4. Set permissions
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&target_bin)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&target_bin, perms)?;
    }

    // 5. Create plist content
    let plist_content = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.clamguard</string>
    <key>ProgramArguments</key>
    <array>
        <string>{target_bin}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>{log_dir}/stdout.log</string>
    <key>StandardErrorPath</key>
    <string>{log_dir}/stderr.log</string>
    <key>ProcessType</key>
    <string>Background</string>
    <key>LowPriorityIO</key>
    <true/>
    <key>Nice</key>
    <integer>5</integer>
</dict>
</plist>"#,
        target_bin = target_bin,
        log_dir = log_dir
    );

    fs::write(&plist_path, plist_content)?;

    // 6. Load service
    // Try to unload first if it was already loaded
    let _ = Command::new("launchctl")
        .arg("unload")
        .arg(&plist_path)
        .status();
    let status = Command::new("launchctl")
        .arg("load")
        .arg("-w")
        .arg(&plist_path)
        .status()?;

    if status.success() {
        info!("Successfully installed and started LaunchAgent");
        Ok(())
    } else {
        anyhow::bail!("Failed to load service with launchctl")
    }
}

pub fn uninstall_service() -> Result<()> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let agent_plist = format!("{}/Library/LaunchAgents/com.clamguard.plist", home);
    let user_bin = format!("{}/Library/Application Support/clamguard/clamguard", home);

    // 1. Unload and remove user agent
    if Path::new(&agent_plist).exists() {
        let _ = Command::new("launchctl")
            .arg("unload")
            .arg("-w")
            .arg(&agent_plist)
            .status();
        let _ = fs::remove_file(&agent_plist);
    }

    // 2. Remove user binary
    if Path::new(&user_bin).exists() {
        let _ = fs::remove_file(&user_bin);
    }

    // 3. Handle legacy root installation
    let legacy_bin = "/usr/local/bin/clamguard";
    let legacy_daemon = "/Library/LaunchDaemons/com.clamguard.plist";

    if Path::new(legacy_bin).exists() || Path::new(legacy_daemon).exists() {
        info!("Legacy root installation detected, requesting privileges to clean up...");
        let script = format!(
            "launchctl unload -w \"{legacy_daemon}\" 2>/dev/null || true; \
             rm -f \"{legacy_daemon}\"; \
             rm -f \"{legacy_bin}\"",
            legacy_daemon = legacy_daemon,
            legacy_bin = legacy_bin
        );
        let osascript = format!(
            "do shell script \"{}\" with administrator privileges",
            script.replace("\"", "\\\"")
        );
        let _ = Command::new("osascript")
            .arg("-e")
            .arg(osascript)
            .status()?;
    }

    info!("Service uninstalled.");
    Ok(())
}

pub fn eject_drive(path: &str) {
    info!("Attempting to eject drive at: {}", path);
    // On macOS, diskutil eject is the standard way to unmount and eject a volume.
    match Command::new("diskutil").arg("eject").arg(path).status() {
        Ok(status) if status.success() => info!("Successfully ejected {}", path),
        Ok(status) => error!("Failed to eject {}: exit code {:?}", path, status.code()),
        Err(e) => error!("Failed to execute diskutil: {}", e),
    }
}

pub fn get_clamav_datadir(is_root: bool) -> Option<String> {
    if let Ok(datadir) = std::env::var("FRESHCLAM_DATADIR") {
        return Some(datadir);
    }

    if is_root {
        None // Use system default
    } else {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        let user_datadir = format!("{}/Library/Caches/clamguard/clamav", home);
        if fs::create_dir_all(&user_datadir).is_ok() {
            Some(user_datadir)
        } else {
            None
        }
    }
}

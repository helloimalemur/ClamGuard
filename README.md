# macOS External Disk ClamAV Scanner

This application runs as a macOS background service, detects when a new external drive is plugged in (via `/Volumes`), and automatically kicks off a ClamAV scan.

It can be installed as a **LaunchAgent** (runs as the current user), which enables the tray icon and does not require administrator privileges for installation.

## Prerequisites

- Rust and Cargo installed.
- ClamAV installed (e.g., via Homebrew: `brew install clamav`).

## Installation & Setup

### Option 1: Via Tray Icon (Recommended)
This is the easiest way to install the application as a **LaunchAgent** (user service), which ensures the tray icon is visible in your menu bar.

1. **Build the application:**
   ```bash
   cargo build --release
   ```
2. **Run the binary:**
   To run without opening a terminal window, it is recommended to build the application bundle:
   ```bash
   make app
   open OSX-Detect-External-Disk.app
   ```
   *Alternatively, run the raw binary (will open Terminal):*
   ```bash
   ./target/release/osx-dek-rs
   ```
3. **Install as Service:** Click the tray icon in the macOS menu bar and select **"Install as Service"**. The application will be copied to `~/Library/Application Support/osx-dek-rs` and registered as a LaunchAgent.

### Option 2: Manual Installation (LaunchAgent - Recommended)

This option installs the application as a `LaunchAgent`, which runs in the user session and allows the tray icon to be visible in the macOS menu bar.

1. **Build the application:**
   ```bash
   cargo build --release
   ```

2. **Install using Makefile:**
   ```bash
   make install
   ```

   This will install the binary to `~/Library/Application Support/osx-dek-rs` and create a LaunchAgent.

   *Alternatively, follow the manual steps below:*

3. **Copy the binary to a user path:**
   ```bash
   mkdir -p "~/Library/Application Support/osx-dek-rs"
   cp target/release/osx-dek-rs "~/Library/Application Support/osx-dek-rs/osx-dek-rs"
   ```

4. **Create the LaunchAgent directory (if it doesn't exist):**
   ```bash
   mkdir -p ~/Library/LaunchAgents
   ```

5. **Copy the Agent Plist:**
   Ensure the plist points to the correct binary location.
   ```bash
   cp com.osx-dek-rs.plist ~/Library/LaunchAgents/com.osx-dek-rs.plist
   ```

6. **Load the Agent:**
   ```bash
   launchctl load -w ~/Library/LaunchAgents/com.osx-dek-rs.plist
   ```

   The tray icon should now appear in your menu bar.

## Features

- **macOS Application Bundle**: Can be built as a native `.app` bundle to run as a GUI-only application without a terminal window.
- **Automatic Scanning**: Detects new mounts in `/Volumes` and scans them with ClamAV.
- **Menu Bar Icon**: Displays a status icon in the macOS menu bar. 
    - **Live Status**: Shows "Idle" or "Active" (with the count of running tasks).
    - **Active Scans**: Lists the paths of all disks currently being scanned.
    - **Install / Uninstall Service**: Allows one-click installation or removal of the persistent system service.
    - **Visual Feedback**: The icon color changes when active (Idle: Gray, Active: Blue).
    - **Quit**: Stops the application.
- **Custom Exclusions**: Supports a `.clamignore` file at the root of the scanned drive. Each line in the file is added to the ClamAV exclusions.
- **Webhook Notifications**: Supports Discord and Slack webhooks to notify when infected items are found.
- **Automatic Reports**: Sends the full ClamAV scan summary in the notification.
- **Automatic Ejection**: Can automatically eject the external drive if infected files are found.
- **Database Updates**: Runs `freshclam` on startup and at a regular interval (default 12 hours) to keep virus definitions current.
- **PCI Audit Logging**: Maintains a dedicated audit log at `/Library/Logs/osx-dek-rs/audit.log` for PCI DSS compliance proof.

## PCI DSS Compliance

This tool is designed to help organizations meet **PCI DSS Requirement 5**: *"Protect all systems against malware and regularly update anti-virus software or programs."*

Specifically, it addresses:
- **Requirement 5.1**: Deploying anti-virus software on all systems commonly affected by malicious software.
- **Requirement 5.2**: Ensuring anti-virus mechanisms are kept current, perform periodic scans, and generate audit logs.
- **Requirement 5.3**: Ensuring anti-virus mechanisms are actively running and cannot be disabled or altered by users (when installed as a `LaunchDaemon`).

### Audit Evidence for PCI
The following files provide the necessary evidence for PCI audits:
1.  **/Library/Logs/osx-dek-rs/audit.log**: A high-level chronological log of all security events, including:
    - Service start/stop.
    - Detection of newly mounted external disks.
    - Start and completion of scans with pass/fail status.
    - Malware detections with infected path details.
    - ClamAV database update attempts and results.
2.  **/Library/Logs/osx-dek-rs/clamav_external_scans.log**: Detailed ClamAV scan reports for every drive scanned.

### Configuration for PCI
For strict PCI compliance, it is recommended to:
1.  Install the application as a **LaunchDaemon** (runs as root, cannot be easily stopped by standard users).
2.  Enable **Automatic Ejection** (`--eject`) to immediately mitigate risk when malware is detected.
3.  Set up **Webhook Notifications** to ensure the security team is alerted immediately upon detection.
4.  Ensure `freshclam` is running correctly (check `audit.log` for `UPDATE_COMPLETE SUCCESS`).

## Configuration

### Toggle Ejection
To enable automatic ejection of infected drives, you can use the `--eject` command-line flag or set the `EJECT_ON_INFECTION` environment variable.

Example with CLI flag:
```bash
/usr/local/bin/osx-dek-rs --eject
```

Example with environment variable:
```bash
export EJECT_ON_INFECTION=true
```

### Service Configuration (LaunchDaemon)
If you want to enable ejection when running as a service, update your `/Library/LaunchDaemons/com.osx-dek-rs.plist` file.

To use the CLI flag, add it to `ProgramArguments`:
```xml
<key>ProgramArguments</key>
<array>
    <string>/usr/local/bin/osx-dek-rs</string>
    <string>--eject</string>
</array>
```

Alternatively, use `EnvironmentVariables`:
```xml
<key>EnvironmentVariables</key>
<dict>
    <key>EJECT_ON_INFECTION</key>
    <string>true</string>
</dict>
```

### .clamignore
You can place a `.clamignore` file at the root of your external drive to exclude specific files or directories from the scan. Each line should contain a pattern (regex supported by ClamAV). Lines starting with `#` are treated as comments.

Example `.clamignore`:
```text
# Exclude large backup folder
/Backups/.*
# Exclude specific file
/some_large_file.iso
```

### Webhook Notifications
To enable notifications, set the following environment variables. You can provide multiple URLs separated by commas.

- `DISCORD_WEBHOOKS`: Comma-separated list of Discord webhook URLs.
- `SLACK_WEBHOOKS`: Comma-separated list of Slack webhook URLs.

Example:
```bash
export DISCORD_WEBHOOKS="https://discord.com/api/webhooks/...,https://discord.com/api/webhooks/..."
```

### Automatic Database Updates (freshclam)
The service automatically runs `freshclam` on startup and every 12 hours by default to keep the virus database up to date.

You can customize the update interval and directories by setting the following environment variables:

- `FRESHCLAM_INTERVAL_HOURS`: Update frequency (default 12 hours). Set to `0` to disable.
- `FRESHCLAM_DATADIR`: Custom directory for the ClamAV virus database.
- `FRESHCLAM_TEMPDIR`: Custom temporary directory for `freshclam` (sets the `TMPDIR` environment variable for the process, useful for resolving permission errors).

Example of setting a 24-hour interval and a custom temp directory in your LaunchDaemon plist:
```xml
<key>EnvironmentVariables</key>
<dict>
    <key>FRESHCLAM_INTERVAL_HOURS</key>
    <string>24</string>
    <key>FRESHCLAM_TEMPDIR</key>
    <string>/tmp</string>
</dict>
```

#### Troubleshooting Freshclam Permissions
If you see errors like `ERROR: Can't create temporary directory`, it usually means the user running `freshclam` (or the `DatabaseOwner` specified in your `freshclam.conf`) doesn't have write access to the default Homebrew database directory.

You can fix this by:
1.  Ensuring `/opt/homebrew/var/lib/clamav` is writable by the `_clamav` user.
2.  Or, pointing `FRESHCLAM_TEMPDIR` to a writable location like `/tmp` in your plist as shown above. This sets the `TMPDIR` environment variable for `freshclam`.
3.  Note: The application now automatically passes `--user=root` to `freshclam` when running as root to help avoid these permission issues.

## Logs

The application uses dynamic log paths based on the user running the service:

- **When running as root (LaunchDaemon):**
    - Audit log: `/Library/Logs/osx-dek-rs/audit.log`
    - Scan outputs: `/Library/Logs/osx-dek-rs/clamav_external_scans.log`
    - Service stdout/stderr: `/Library/Logs/osx-dek-rs/stdout.log` and `stderr.log`
- **When running as a user (LaunchAgent):**
    - Audit log: `~/Library/Logs/osx-dek-rs/audit.log`
    - Scan outputs: `~/Library/Logs/osx-dek-rs/clamav_external_scans.log`
    - Service stdout/stderr: `/tmp/osx-dek-rs.agent.stdout.log` and `stderr.log`

*Note: If the preferred log directory is not writable, the application will fallback to creating `audit_fallback.log` and `clamav_external_scans.log` in the current working directory.*

## Permission Issues & Full Disk Access

When running as a `LaunchAgent` (as a normal user), the application may encounter permission issues when trying to scan certain files on external disks that are owned by other users or the system.

To ensure comprehensive scanning, you should grant **Full Disk Access** to the binary:
1. Open **System Settings** > **Privacy & Security** > **Full Disk Access**.
2. Click the **+** button.
3. Press `Cmd + Shift + G` and type `/usr/local/bin/osx-dek-rs`.
4. Ensure the toggle is turned **ON**.

When running as a `LaunchDaemon` (as `root`), the application generally has the necessary permissions, but Full Disk Access is still recommended on newer macOS versions for complete coverage.

## Removal & Uninstallation

To completely remove the application and its service configurations, you can use the following methods:

### Option 1: Via Tray Icon
If the application is running as a LaunchAgent, you can click the tray icon and select **"Uninstall Service"**. You will be prompted for your administrator password.

### Option 2: Manual Removal (Recommended)
This method uses the Makefile to clean up both LaunchAgent and LaunchDaemon configurations.

```bash
sudo make uninstall
```

### Option 3: Manual Step-by-Step Removal

If you prefer to remove files manually, follow these steps:

1. **Unload and remove LaunchAgent (if installed):**
   ```bash
   launchctl unload -w ~/Library/LaunchAgents/com.osx-dek-rs.plist
   rm ~/Library/LaunchAgents/com.osx-dek-rs.plist
   ```

2. **Unload and remove LaunchDaemon (if installed):**
   ```bash
   sudo launchctl unload -w /Library/LaunchDaemons/com.osx-dek-rs.plist
   sudo rm /Library/LaunchDaemons/com.osx-dek-rs.plist
   ```

3. **Remove the binary:**
   ```bash
   sudo rm /usr/local/bin/osx-dek-rs
   ```

4. **(Optional) Remove logs:**
   ```bash
   rm -rf ~/Library/Logs/osx-dek-rs/
   sudo rm -rf /Library/Logs/osx-dek-rs/
   ```

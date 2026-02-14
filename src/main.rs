mod audit;
mod config;
mod guard;
mod gui;
mod history;
mod notifications;
mod scanner;
mod utils;

use anyhow::Result;
use log::{debug, error, info, warn};
use notify::{Config as NotifyConfig, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};

use crate::config::Config;
use crate::guard::{AppState, ScanGuard};
use crate::scanner::{run_clamscan, run_freshclam};
use crate::utils::{
    IconState, create_icon, find_clamscan, find_freshclam, install_as_service,
    is_service_installed, uninstall_service,
};

#[cfg(target_os = "macos")]
#[unsafe(link_section = "__TEXT,__info_plist")]
#[used]
static INFO_PLIST: [u8; include_bytes!("../Info.plist").len()] = *include_bytes!("../Info.plist");

#[cfg(target_os = "macos")]
fn hide_dock_icon() {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
    if let Some(mtm) = MainThreadMarker::new() {
        let app = NSApplication::sharedApplication(mtm);
        debug!("Setting macOS activation policy to Accessory");
        let _ = app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    }
}

use crate::gui::SettingsApp;
use crate::history::ScanStatus;
use chrono::{Datelike, Timelike};
use std::sync::atomic::Ordering;
use tray_icon::{
    TrayIcon, TrayIconBuilder,
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
};

fn create_tray_menu(
    app_state: &AppState,
) -> (
    Menu,
    MenuItem,
    MenuItem,
    MenuItem,
    MenuItem,
    MenuItem,
    MenuItem,
    MenuItem,
    Option<MenuItem>,
) {
    let menu = Menu::new();

    let mut dismiss_item = None;
    if app_state.infection_found.load(Ordering::SeqCst) {
        let item = MenuItem::new("⚠️ Dismiss Virus Warning", true, None);
        let _ = menu.append(&item);
        let _ = menu.append(&PredefinedMenuItem::separator());
        dismiss_item = Some(item);
    }

    let active_tasks = app_state.active_tasks.load(Ordering::SeqCst);
    let status_text = if active_tasks > 0 {
        format!("Status: Active ({})", active_tasks)
    } else {
        "Status: Idle".to_string()
    };
    let status_item = MenuItem::new(status_text, false, None);
    let _ = menu.append(&status_item);

    let _ = menu.append(&PredefinedMenuItem::separator());

    {
        let scans = app_state.active_scans.lock().unwrap();
        if scans.is_empty() {
            let _ = menu.append(&MenuItem::new("No active scans", false, None));
        } else {
            let _ = menu.append(&MenuItem::new("Active Scans:", false, None));
            for path in scans.keys() {
                // Shorten the path for the menu if needed, but for now just show it
                let _ = menu.append(&MenuItem::new(format!("  {}", path), false, None));
            }
        }
    }

    let _ = menu.append(&PredefinedMenuItem::separator());

    let urls_item = MenuItem::new("Configure Webhooks & Settings...", true, None);
    let _ = menu.append(&urls_item);

    let history_item = MenuItem::new("Show Scan History...", true, None);
    let _ = menu.append(&history_item);

    let scheduling_item = MenuItem::new("Scheduling Settings...", true, None);
    let _ = menu.append(&scheduling_item);

    let scan_item = MenuItem::new("Start Custom Scan...", true, None);
    let _ = menu.append(&scan_item);

    let installed = is_service_installed();
    let install_item = MenuItem::new("Install as Service", !installed, None);
    let _ = menu.append(&install_item);

    let uninstall_item = MenuItem::new("Uninstall Service", installed, None);
    let _ = menu.append(&uninstall_item);

    let quit_item = MenuItem::new("Quit", true, None);
    let _ = menu.append(&quit_item);

    (
        menu,
        urls_item,
        history_item,
        scheduling_item,
        scan_item,
        install_item,
        uninstall_item,
        quit_item,
        dismiss_item,
    )
}

fn main() -> Result<()> {
    // Initialize logger to write to stdout/stderr.
    env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));

    #[cfg(target_os = "macos")]
    hide_dock_icon();

    let config = Config::load();
    let history = crate::history::History::load();

    let app_state = Arc::new(AppState {
        active_tasks: std::sync::atomic::AtomicUsize::new(0),
        infection_found: std::sync::atomic::AtomicBool::new(false),
        active_scans: Mutex::new(HashMap::new()),
        egui_ctx: Mutex::new(None),
        config: Mutex::new(config.clone()),
        history: Mutex::new(history),
        export_status: Mutex::new(None),
    });

    let args: Vec<String> = std::env::args().collect();
    if args.contains(&"--version".to_string()) || args.contains(&"-v".to_string()) {
        println!("clamguard version {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    let eject_enabled = args.contains(&"--eject".to_string()) || config.eject_on_infection;

    if eject_enabled {
        info!("Ejection on infection is ENABLED");
    } else {
        info!("Ejection on infection is DISABLED");
    }

    info!("Starting macOS External Disk Detector and ClamAV Scanner Service");

    // Spawn the background service logic
    let app_state_clone = Arc::clone(&app_state);
    std::thread::spawn(move || {
        if let Err(e) = run_service(app_state_clone) {
            error!("Service error: {}", e);
        }
    });

    // Spawn the scheduler logic
    let app_state_scheduler = Arc::clone(&app_state);
    std::thread::spawn(move || {
        run_scheduler(app_state_scheduler);
    });

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([450.0, 450.0])
            .with_visible(false)
            .with_title("ClamGuard Settings"),
        event_loop_builder: Some(Box::new(|_builder| {
            #[cfg(target_os = "macos")]
            hide_dock_icon();
        })),
        ..Default::default()
    };

    eframe::run_native(
        "ClamGuard",
        options,
        Box::new(|cc| Ok(Box::new(DekApp::new(cc, app_state)))),
    )
    .map_err(|e| anyhow::anyhow!("Eframe error: {}", e))
}

struct DekApp {
    settings: SettingsApp,
    tray: TrayIcon,
    quit_id: tray_icon::menu::MenuId,
    install_id: tray_icon::menu::MenuId,
    uninstall_id: tray_icon::menu::MenuId,
    settings_id: tray_icon::menu::MenuId,
    history_id: tray_icon::menu::MenuId,
    scheduling_id: tray_icon::menu::MenuId,
    scan_id: tray_icon::menu::MenuId,
    dismiss_id: Option<tray_icon::menu::MenuId>,
    last_infection_state: bool,
    last_active_tasks: usize,
}

impl DekApp {
    fn new(cc: &eframe::CreationContext<'_>, app_state: Arc<AppState>) -> Self {
        #[cfg(target_os = "macos")]
        hide_dock_icon();

        if let Ok(mut ctx_lock) = app_state.egui_ctx.lock() {
            *ctx_lock = Some(cc.egui_ctx.clone());
        }

        let (
            menu,
            settings_item,
            history_item,
            scheduling_item,
            scan_item,
            install_item,
            uninstall_item,
            quit_item,
            dismiss_item,
        ) = create_tray_menu(&app_state);

        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_icon(create_icon(IconState::Idle))
            .with_tooltip("ClamGuard")
            .build()
            .unwrap();

        let infection_state = app_state.infection_found.load(Ordering::SeqCst);
        let active_tasks = app_state.active_tasks.load(Ordering::SeqCst);

        Self {
            settings: SettingsApp::new(app_state),
            tray,
            quit_id: quit_item.id().clone(),
            install_id: install_item.id().clone(),
            uninstall_id: uninstall_item.id().clone(),
            settings_id: settings_item.id().clone(),
            history_id: history_item.id().clone(),
            scheduling_id: scheduling_item.id().clone(),
            scan_id: scan_item.id().clone(),
            dismiss_id: dismiss_item.map(|i| i.id().clone()),
            last_infection_state: infection_state,
            last_active_tasks: active_tasks,
        }
    }

    fn refresh_tray(&mut self) {
        let (
            menu,
            settings_item,
            history_item,
            scheduling_item,
            scan_item,
            install_item,
            uninstall_item,
            quit_item,
            dismiss_item,
        ) = create_tray_menu(&self.settings.app_state);
        let _ = self.tray.set_menu(Some(Box::new(menu)));
        self.quit_id = quit_item.id().clone();
        self.install_id = install_item.id().clone();
        self.uninstall_id = uninstall_item.id().clone();
        self.settings_id = settings_item.id().clone();
        self.history_id = history_item.id().clone();
        self.scheduling_id = scheduling_item.id().clone();
        self.scan_id = scan_item.id().clone();
        self.dismiss_id = dismiss_item.map(|i| i.id().clone());
    }
}

impl eframe::App for DekApp {
    fn update(&mut self, ctx: &eframe::egui::Context, frame: &mut eframe::Frame) {
        // Handle Tray Events
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            if event.id == self.quit_id {
                info!("Quit menu item clicked");
                audit::log_service_stop();
                self.settings.quitting = true;
                ctx.send_viewport_cmd(eframe::egui::ViewportCommand::Close);
            } else if event.id == self.settings_id {
                info!("Settings menu item clicked");
                self.settings.current_tab = crate::gui::GuiTab::Settings;
                self.settings.visible = true;
                ctx.send_viewport_cmd(eframe::egui::ViewportCommand::Focus);
            } else if event.id == self.scan_id {
                info!("Custom Scan menu item clicked");
                if let Some(path) = rfd::FileDialog::new().pick_folder() {
                    let path_str = path.to_string_lossy().to_string();
                    info!("Starting custom scan for: {}", path_str);

                    let app_state = Arc::clone(&self.settings.app_state);
                    let eject_on_infection = app_state.config.lock().unwrap().eject_on_infection;

                    app_state.add_scan(path_str.clone());
                    let app_state_task = Arc::clone(&app_state);
                    let path_str_task = path_str.clone();
                    std::thread::spawn(move || {
                        let _guard = ScanGuard {
                            path: path_str_task.clone(),
                            app_state: Arc::clone(&app_state_task),
                        };

                        match run_clamscan(
                            Arc::clone(&app_state_task),
                            &path_str_task,
                            eject_on_infection,
                        ) {
                            Ok((infected, summary, infected_files)) => {
                                let status = if infected {
                                    ScanStatus::Infected
                                } else {
                                    ScanStatus::Clean
                                };
                                _guard.app_state.add_history_entry(
                                    path_str,
                                    status,
                                    summary,
                                    infected_files,
                                );
                                if infected {
                                    _guard.app_state.report_infection();
                                }
                            }
                            Err(e) => {
                                let err_msg = e.to_string();
                                error!("Custom ClamAV scan failed for {}: {}", path_str, err_msg);
                                _guard.app_state.add_history_entry(
                                    path_str,
                                    ScanStatus::Failed,
                                    err_msg,
                                    Vec::new(),
                                );
                            }
                        }
                    });
                }
            } else if event.id == self.history_id {
                info!("History menu item clicked");
                self.settings.current_tab = crate::gui::GuiTab::History;
                self.settings.visible = true;
                ctx.send_viewport_cmd(eframe::egui::ViewportCommand::Focus);
            } else if event.id == self.scheduling_id {
                info!("Scheduling menu item clicked");
                self.settings.current_tab = crate::gui::GuiTab::Scheduling;
                self.settings.visible = true;
                ctx.send_viewport_cmd(eframe::egui::ViewportCommand::Focus);
            } else if event.id == self.install_id {
                info!("Install Service clicked");
                let _ = install_as_service();
                self.refresh_tray();
            } else if event.id == self.uninstall_id {
                info!("Uninstall Service clicked");
                let _ = uninstall_service();
                self.refresh_tray();
            } else if let Some(ref d_id) = self.dismiss_id {
                if event.id == *d_id {
                    info!("Dismiss Warning clicked");
                    self.settings.app_state.clear_infection();
                    self.refresh_tray();
                }
            }
        }

        // Check if state changed to update icon and menu
        let infection_state = self
            .settings
            .app_state
            .infection_found
            .load(Ordering::SeqCst);
        let active_tasks = self.settings.app_state.active_tasks.load(Ordering::SeqCst);

        if infection_state != self.last_infection_state || active_tasks != self.last_active_tasks {
            let state = if infection_state {
                IconState::Infected
            } else if active_tasks > 0 {
                IconState::Active
            } else {
                IconState::Idle
            };
            let _ = self.tray.set_icon(Some(create_icon(state)));
            self.refresh_tray();
            self.last_infection_state = infection_state;
            self.last_active_tasks = active_tasks;
        }

        // Delegate to settings app
        self.settings.update(ctx, frame);

        // Ensure we poll for tray events even when window is hidden
        if !self.settings.visible {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
    }
}

fn run_service(app_state: Arc<AppState>) -> Result<()> {
    audit::log_service_start();
    let clamscan_path = find_clamscan();
    if clamscan_path == "clamscan" {
        if Command::new("clamscan").arg("--version").output().is_err() {
            let err_msg = "clamscan not found in common paths or PATH. Please install ClamAV.";
            error!("{}", err_msg);
            audit::log_error("CRITICAL", err_msg);
        } else {
            info!("clamscan found in PATH");
        }
    } else {
        info!("clamscan found at: {}", clamscan_path);
    }

    let freshclam_path = find_freshclam();
    if freshclam_path == "freshclam" {
        if Command::new("freshclam").arg("--version").output().is_err() {
            let warn_msg =
                "freshclam not found in common paths or PATH. Database updates might fail.";
            warn!("{}", warn_msg);
            audit::log_error("WARNING", warn_msg);
        } else {
            info!("freshclam found in PATH");
        }
    } else {
        info!("freshclam found at: {}", freshclam_path);
    }

    // Freshclam setup
    let current_config = app_state.config.lock().unwrap().clone();
    let fresh_interval_hours = current_config.freshclam_interval_hours as u64;

    if fresh_interval_hours > 0 {
        info!("Freshclam interval set to {} hours", fresh_interval_hours);

        // Run freshclam on start
        app_state.increment();
        if let Err(e) = run_freshclam() {
            error!("Initial freshclam update failed: {}", e);
        }
        app_state.decrement();

        // Spawn periodic freshclam thread
        let app_state_clone = Arc::clone(&app_state);
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(std::time::Duration::from_secs(fresh_interval_hours * 3600));
                info!("Running periodic freshclam update...");
                app_state_clone.increment();
                if let Err(e) = run_freshclam() {
                    error!("Periodic freshclam update failed: {}", e);
                }
                app_state_clone.decrement();
            }
        });
    } else {
        info!("Freshclam automatic updates are disabled (interval set to 0)");
    }

    let (tx, rx) = std::sync::mpsc::channel();
    let mut watcher = RecommendedWatcher::new(tx, NotifyConfig::default())?;
    let volumes_path = Path::new("/Volumes");
    watcher.watch(volumes_path, RecursiveMode::NonRecursive)?;

    info!("Watching /Volumes for new mounts...");

    let mut scanned_paths: HashMap<String, u64> = HashMap::new();

    for res in rx {
        match res {
            Ok(event) => {
                debug!("Received event: {:?}", event);
                scanned_paths.retain(|path_str, _| Path::new(path_str).exists());

                if event.kind.is_create() {
                    for path in event.paths {
                        if let Some(path_str) = path.to_str() {
                            if path
                                .file_name()
                                .and_then(|n| n.to_str())
                                .map(|s| s.starts_with('.'))
                                .unwrap_or(false)
                            {
                                continue;
                            }

                            let current_dev = std::fs::metadata(&path).map(|m| m.dev()).ok();
                            if let Some(dev_id) = current_dev {
                                if let Some(&stored_dev) = scanned_paths.get(path_str) {
                                    if dev_id == stored_dev {
                                        debug!("Skipping {}, already scanned", path_str);
                                        continue;
                                    }
                                }

                                {
                                    let active = app_state.active_scans.lock().unwrap();
                                    if active.contains_key(path_str) {
                                        info!(
                                            "Scan already in progress for {}, skipping",
                                            path_str
                                        );
                                        continue;
                                    }
                                }

                                info!("Detected new mount in /Volumes: {}", path_str);
                                audit::log_event(
                                    "MOUNT_DETECTED",
                                    &format!("New mount detected: {}", path_str),
                                );
                                scanned_paths.insert(path_str.to_string(), dev_id);

                                let path_to_scan = path_str.to_string();
                                let app_state_clone = Arc::clone(&app_state);

                                app_state.add_scan(path_to_scan.clone());
                                std::thread::spawn(move || {
                                    let _guard = ScanGuard {
                                        path: path_to_scan.clone(),
                                        app_state: app_state_clone,
                                    };

                                    std::thread::sleep(std::time::Duration::from_secs(2));

                                    let path = std::path::Path::new(&path_to_scan);
                                    if path.exists() && path.is_dir() {
                                        info!("Starting ClamAV scan for: {}", path_to_scan);
                                        let current_eject = _guard
                                            .app_state
                                            .config
                                            .lock()
                                            .unwrap()
                                            .eject_on_infection;
                                        match run_clamscan(
                                            Arc::clone(&_guard.app_state),
                                            &path_to_scan,
                                            current_eject,
                                        ) {
                                            Ok((infected, summary, infected_files)) => {
                                                info!("ClamAV scan completed for {}", path_to_scan);
                                                let status = if infected {
                                                    ScanStatus::Infected
                                                } else {
                                                    ScanStatus::Clean
                                                };
                                                _guard.app_state.add_history_entry(
                                                    path_to_scan,
                                                    status,
                                                    summary,
                                                    infected_files,
                                                );
                                                if infected {
                                                    _guard.app_state.report_infection();
                                                }
                                            }
                                            Err(e) => {
                                                let err_msg = e.to_string();
                                                error!(
                                                    "ClamAV scan failed for {}: {}",
                                                    path_to_scan, err_msg
                                                );
                                                _guard.app_state.add_history_entry(
                                                    path_to_scan,
                                                    ScanStatus::Failed,
                                                    err_msg,
                                                    Vec::new(),
                                                );
                                            }
                                        }
                                    }
                                });
                            }
                        }
                    }
                } else if event.kind.is_remove() {
                    for path in event.paths {
                        if let Some(path_str) = path.to_str() {
                            if scanned_paths.remove(path_str).is_some() {
                                info!(
                                    "Entry removed from /Volumes, cleared from scanned cache: {}",
                                    path_str
                                );
                            }
                        }
                    }
                }
            }
            Err(e) => error!("watch error: {:?}", e),
        }
    }

    Ok(())
}

fn run_scheduler(app_state: Arc<AppState>) {
    use crate::config::ScheduleInterval;
    use chrono::Local;

    info!("Scheduler thread started");

    loop {
        // Check every minute
        std::thread::sleep(std::time::Duration::from_secs(60));

        let config = {
            let config_lock = app_state.config.lock().unwrap();
            config_lock.clone()
        };

        if config.scheduled_scan_interval == ScheduleInterval::None {
            continue;
        }

        let now = Local::now();
        let today = now.date_naive();

        // Parse time
        let parts: Vec<&str> = config.scheduled_scan_time.split(':').collect();
        if parts.len() != 2 {
            continue;
        }
        let hour: u32 = parts[0].parse().unwrap_or(2);
        let minute: u32 = parts[1].parse().unwrap_or(0);

        if now.hour() == hour && now.minute() == minute {
            let should_run = match config.scheduled_scan_interval {
                ScheduleInterval::Daily => true,
                ScheduleInterval::Weekly => {
                    now.weekday().num_days_from_sunday() == config.scheduled_scan_day
                }
                ScheduleInterval::None => false,
            };

            if should_run {
                // Check if we already ran a system scan today (to avoid double scans if app restarts)
                let already_ran = {
                    let history = app_state.history.lock().unwrap();
                    history.entries.iter().any(|e| {
                        e.path == "/"
                            && e.timestamp.date_naive() == today
                            && e.status != ScanStatus::Failed
                    })
                };

                if already_ran {
                    debug!("Scheduled scan for today already completed, skipping");
                    continue;
                }

                info!("Starting scheduled system scan...");

                let app_state_clone = Arc::clone(&app_state);
                let eject_on_infection = config.eject_on_infection;
                let path_to_scan = "/".to_string();

                app_state.add_scan(path_to_scan.clone());
                std::thread::spawn(move || {
                    let _guard = ScanGuard {
                        path: path_to_scan.clone(),
                        app_state: Arc::clone(&app_state_clone),
                    };

                    match run_clamscan(
                        Arc::clone(&_guard.app_state),
                        &path_to_scan,
                        eject_on_infection,
                    ) {
                        Ok((infected, summary, infected_files)) => {
                            let status = if infected {
                                ScanStatus::Infected
                            } else {
                                ScanStatus::Clean
                            };
                            _guard.app_state.add_history_entry(
                                path_to_scan,
                                status,
                                summary,
                                infected_files,
                            );
                            if infected {
                                _guard.app_state.report_infection();
                            }
                        }
                        Err(e) => {
                            let err_msg = e.to_string();
                            error!("Scheduled ClamAV scan failed: {}", err_msg);
                            _guard.app_state.add_history_entry(
                                path_to_scan,
                                ScanStatus::Failed,
                                err_msg,
                                Vec::new(),
                            );
                        }
                    }
                });

                // Sleep for a bit more to ensure we don't trigger again in the same minute
                std::thread::sleep(std::time::Duration::from_secs(61));
            }
        }
    }
}

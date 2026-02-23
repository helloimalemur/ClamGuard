use crate::config::Config;
use crate::history::ScanStatus;
use eframe::egui;
use std::sync::Arc;

#[derive(PartialEq)]
pub enum GuiTab {
    Settings,
    ActiveScans,
    History,
    Scheduling,
}

pub struct SettingsApp {
    pub config: Config,
    pub app_state: Arc<crate::guard::AppState>,
    pub visible: bool,
    pub current_tab: GuiTab,
    pub quitting: bool,
    pub status_message: Option<(String, egui::Color32, std::time::Instant)>,
    last_visible: bool,
}

impl SettingsApp {
    pub fn new(app_state: Arc<crate::guard::AppState>) -> Self {
        let config = app_state.config.lock().unwrap().clone();
        Self {
            config,
            app_state,
            visible: false,
            current_tab: GuiTab::Settings,
            quitting: false,
            status_message: None,
            last_visible: true,
        }
    }

    pub fn save_config(&self) {
        let mut config = self.app_state.config.lock().unwrap();
        *config = self.config.clone();
        if let Err(e) = config.save() {
            log::error!("Failed to save config: {}", e);
        }
    }
}

impl eframe::App for SettingsApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Handle visibility
        if self.visible != self.last_visible {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(self.visible));
            if self.visible {
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
            self.last_visible = self.visible;
        }

        if self.visible {
            // Check for background export results
            if let Ok(mut status_lock) = self.app_state.export_status.lock() {
                if let Some(result) = status_lock.take() {
                    match result {
                        Ok(path) => {
                            log::info!("History successfully exported to {:?}", path);
                            self.status_message = Some((
                                format!(
                                    "Exported to {}",
                                    path.file_name().and_then(|n| n.to_str()).unwrap_or("file")
                                ),
                                egui::Color32::GREEN,
                                std::time::Instant::now(),
                            ));
                        }
                        Err(e) => {
                            log::error!("Failed to export CSV: {}", e);
                            self.status_message = Some((
                                format!("Export failed: {}", e),
                                egui::Color32::RED,
                                std::time::Instant::now(),
                            ));
                        }
                    }
                }
            }

            egui::CentralPanel::default().show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.current_tab, GuiTab::Settings, "Settings");
                    ui.selectable_value(&mut self.current_tab, GuiTab::ActiveScans, "Active Scans");
                    ui.selectable_value(&mut self.current_tab, GuiTab::History, "Scan History");
                    ui.selectable_value(&mut self.current_tab, GuiTab::Scheduling, "Scheduling");
                });
                ui.separator();

                match self.current_tab {
                    GuiTab::Settings => self.show_settings(ui),
                    GuiTab::ActiveScans => self.show_active_scans(ui),
                    GuiTab::History => self.show_history(ui),
                    GuiTab::Scheduling => self.show_scheduling(ui),
                }

                if let Some((msg, color, time)) = &self.status_message {
                    if time.elapsed().as_secs() < 5 {
                        ui.add_space(10.0);
                        ui.colored_label(*color, msg);
                    }
                }
            });
        }

        // Close window if the viewport close button is clicked
        if ctx.input(|i| i.viewport().close_requested()) {
            if !self.quitting {
                self.visible = false;
                // Discard changes and reload
                self.config = self.app_state.config.lock().unwrap().clone();
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            }
        }
    }
}

impl SettingsApp {
    fn show_settings(&mut self, ui: &mut egui::Ui) {
        ui.heading("ClamGuard Settings");
        ui.separator();

        ui.add_space(10.0);

        ui.group(|ui| {
            ui.label("General Settings");
            ui.checkbox(
                &mut self.config.eject_on_infection,
                "Eject disk immediately on infection found",
            );
            ui.add_space(5.0);
            ui.checkbox(
                &mut self.config.show_uninstall_button,
                "Show 'Uninstall Service' in tray menu",
            );
            ui.checkbox(
                &mut self.config.show_quit_button,
                "Show 'Quit' in tray menu",
            );
        });

        ui.add_space(10.0);

        ui.group(|ui| {
            ui.label("Notifications (Webhooks)");

            ui.horizontal(|ui| {
                ui.label("Discord Webhook:");
                ui.text_edit_singleline(&mut self.config.discord_webhooks);
            });
            ui.add_space(5.0);
            ui.horizontal(|ui| {
                ui.label("Slack Webhook:");
                ui.text_edit_singleline(&mut self.config.slack_webhooks);
            });
        });

        ui.add_space(10.0);

        ui.group(|ui| {
            ui.label("Scanner Settings");
            ui.horizontal(|ui| {
                ui.label("Freshclam update interval (hours):");
                ui.add(
                    egui::DragValue::new(&mut self.config.freshclam_interval_hours).range(1..=168),
                );
            });
            ui.small("Interval for updating ClamAV virus definitions.");
        });

        ui.add_space(20.0);

        ui.horizontal(|ui| {
            if ui.button("Save & Apply").clicked() {
                self.save_config();
                self.visible = false;
            }

            if ui.button("Cancel").clicked() {
                // Reload config from state to discard changes
                self.config = self.app_state.config.lock().unwrap().clone();
                self.visible = false;
            }
        });
    }

    fn show_scheduling(&mut self, ui: &mut egui::Ui) {
        ui.heading("Scheduled System Scans");
        ui.separator();

        ui.add_space(10.0);

        ui.group(|ui| {
            ui.label("Schedule Frequency");
            ui.horizontal(|ui| {
                ui.selectable_value(
                    &mut self.config.scheduled_scan_interval,
                    crate::config::ScheduleInterval::None,
                    "None",
                );
                ui.selectable_value(
                    &mut self.config.scheduled_scan_interval,
                    crate::config::ScheduleInterval::Daily,
                    "Daily",
                );
                ui.selectable_value(
                    &mut self.config.scheduled_scan_interval,
                    crate::config::ScheduleInterval::Weekly,
                    "Weekly",
                );
            });
        });

        if self.config.scheduled_scan_interval != crate::config::ScheduleInterval::None {
            ui.add_space(10.0);
            ui.group(|ui| {
                ui.label("Scan Time");
                ui.horizontal(|ui| {
                    ui.label("Time (HH:MM):");
                    ui.text_edit_singleline(&mut self.config.scheduled_scan_time);
                });
                ui.small("Use 24-hour format (e.g., 02:00 or 14:30).");

                if self.config.scheduled_scan_interval == crate::config::ScheduleInterval::Weekly {
                    ui.add_space(5.0);
                    ui.horizontal(|ui| {
                        ui.label("Day of week:");
                        egui::ComboBox::from_label("")
                            .selected_text(match self.config.scheduled_scan_day {
                                0 => "Sunday",
                                1 => "Monday",
                                2 => "Tuesday",
                                3 => "Wednesday",
                                4 => "Thursday",
                                5 => "Friday",
                                6 => "Saturday",
                                _ => "Unknown",
                            })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut self.config.scheduled_scan_day,
                                    0,
                                    "Sunday",
                                );
                                ui.selectable_value(
                                    &mut self.config.scheduled_scan_day,
                                    1,
                                    "Monday",
                                );
                                ui.selectable_value(
                                    &mut self.config.scheduled_scan_day,
                                    2,
                                    "Tuesday",
                                );
                                ui.selectable_value(
                                    &mut self.config.scheduled_scan_day,
                                    3,
                                    "Wednesday",
                                );
                                ui.selectable_value(
                                    &mut self.config.scheduled_scan_day,
                                    4,
                                    "Thursday",
                                );
                                ui.selectable_value(
                                    &mut self.config.scheduled_scan_day,
                                    5,
                                    "Friday",
                                );
                                ui.selectable_value(
                                    &mut self.config.scheduled_scan_day,
                                    6,
                                    "Saturday",
                                );
                            });
                    });
                }
            });
        }

        ui.add_space(20.0);
        ui.label("Scheduled scans will perform a full scan of the system root (/).");

        ui.add_space(20.0);

        ui.horizontal(|ui| {
            if ui.button("Save & Apply").clicked() {
                self.save_config();
                self.visible = false;
            }

            if ui.button("Cancel").clicked() {
                // Reload config from state to discard changes
                self.config = self.app_state.config.lock().unwrap().clone();
                self.visible = false;
            }
        });
    }

    fn show_active_scans(&mut self, ui: &mut egui::Ui) {
        ui.heading("Active Scans");
        ui.separator();

        ui.add_space(10.0);

        let mut to_cancel = None;
        {
            let scans = self.app_state.active_scans.lock().unwrap();
            if scans.is_empty() {
                ui.label("No active scans in progress.");
            } else {
                for (path, state) in scans.iter() {
                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.label(format!("Path: {}", path));
                                ui.small(format!("Scanning: {}", state.current_file))
                                    .on_hover_text(&state.current_file);
                            });

                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.button("Cancel Scan").clicked() {
                                        to_cancel = Some(path.clone());
                                    }
                                },
                            );
                        });
                    });
                    ui.add_space(5.0);
                }
            }
        }

        if let Some(path) = to_cancel {
            self.app_state.cancel_scan(&path);
        }
    }

    fn show_history(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Scan & Result History");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Export to CSV").clicked() {
                    log::info!("Export to CSV button clicked");
                    if let Some(path) = rfd::FileDialog::new()
                        .set_file_name("scan_history.csv")
                        .add_filter("CSV", &["csv"])
                        .save_file()
                    {
                        log::info!("User selected export path: {:?}", path);
                        self.status_message = Some((
                            "Exporting... please wait".to_string(),
                            egui::Color32::YELLOW,
                            std::time::Instant::now(),
                        ));

                        let app_state = Arc::clone(&self.app_state);
                        let path_clone = path.clone();
                        std::thread::spawn(move || {
                            let result = (|| -> anyhow::Result<std::path::PathBuf> {
                                let history = app_state.history.lock().unwrap();
                                let csv_content = history.to_csv()?;
                                std::fs::write(&path_clone, csv_content)?;
                                Ok(path_clone)
                            })();

                            if let Ok(mut status_lock) = app_state.export_status.lock() {
                                *status_lock = Some(result);
                            }

                            // Trigger repaint to show the result
                            if let Ok(ctx_lock) = app_state.egui_ctx.lock() {
                                if let Some(ctx) = ctx_lock.as_ref() {
                                    ctx.request_repaint();
                                }
                            }
                        });
                    } else {
                        log::info!("Export to CSV cancelled by user (no path selected)");
                    }
                }
            });
        });
        ui.separator();

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .stick_to_bottom(false)
            .show(ui, |ui| {
                let history = self.app_state.history.lock().unwrap();
                if history.entries.is_empty() {
                    ui.label("No scan history recorded yet.");
                } else {
                    for entry in history.entries.iter().rev() {
                        ui.group(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(entry.timestamp.format("%Y-%m-%d %H:%M:%S").to_string());
                                ui.separator();
                                match entry.status {
                                    ScanStatus::Clean => {
                                        ui.colored_label(egui::Color32::GREEN, "CLEAN")
                                    }
                                    ScanStatus::Infected => {
                                        ui.colored_label(egui::Color32::RED, "INFECTED")
                                    }
                                    ScanStatus::Failed => {
                                        ui.colored_label(egui::Color32::YELLOW, "FAILED")
                                    }
                                };
                            });
                            ui.label(format!("Path: {}", entry.path));

                            if !entry.infected_files.is_empty() {
                                let infected_id = ui.make_persistent_id(format!(
                                    "infected_{}",
                                    entry.timestamp.timestamp_nanos_opt().unwrap_or(0)
                                ));
                                egui::collapsing_header::CollapsingState::load_with_default_open(
                                    ui.ctx(),
                                    infected_id,
                                    false,
                                )
                                .show_header(ui, |ui| {
                                    ui.colored_label(
                                        egui::Color32::LIGHT_RED,
                                        format!(
                                            "⚠️ Infected Files ({})",
                                            entry.infected_files.len()
                                        ),
                                    );
                                })
                                .body(|ui| {
                                    for file in &entry.infected_files {
                                        ui.horizontal(|ui| {
                                            ui.add_space(10.0);
                                            ui.small(file);
                                        });
                                    }
                                });
                            }

                            if !entry.details.is_empty() {
                                let details_id = ui.make_persistent_id(format!(
                                    "details_{}",
                                    entry.timestamp.timestamp_nanos_opt().unwrap_or(0)
                                ));
                                egui::collapsing_header::CollapsingState::load_with_default_open(
                                    ui.ctx(),
                                    details_id,
                                    false,
                                )
                                .show_header(ui, |ui| {
                                    ui.label("Details");
                                })
                                .body(|ui| {
                                    ui.label(&entry.details);
                                });
                            }
                        });
                        ui.add_space(5.0);
                    }
                }
            });
    }
}

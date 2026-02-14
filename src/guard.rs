use crate::config::Config;
use crate::history::{History, ScanStatus};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

pub struct ScanState {
    pub current_file: String,
    pub cancel_flag: Arc<AtomicBool>,
}

pub struct AppState {
    pub active_tasks: AtomicUsize,
    pub infection_found: AtomicBool,
    pub active_scans: Mutex<HashMap<String, ScanState>>,
    pub egui_ctx: Mutex<Option<eframe::egui::Context>>,
    pub config: Mutex<Config>,
    pub history: Mutex<History>,
    pub export_status: Mutex<Option<anyhow::Result<std::path::PathBuf>>>,
}

impl AppState {
    pub fn increment(&self) {
        self.active_tasks.fetch_add(1, Ordering::SeqCst);
        if let Ok(ctx_opt) = self.egui_ctx.lock() {
            if let Some(ctx) = ctx_opt.as_ref() {
                ctx.request_repaint();
            }
        }
    }

    pub fn decrement(&self) {
        let prev = self.active_tasks.fetch_sub(1, Ordering::SeqCst);
        if prev == 0 {
            // Should not happen, but reset to 0
            self.active_tasks.store(0, Ordering::SeqCst);
        }
        if let Ok(ctx_opt) = self.egui_ctx.lock() {
            if let Some(ctx) = ctx_opt.as_ref() {
                ctx.request_repaint();
            }
        }
    }

    pub fn report_infection(&self) {
        self.infection_found.store(true, Ordering::SeqCst);
        if let Ok(ctx_opt) = self.egui_ctx.lock() {
            if let Some(ctx) = ctx_opt.as_ref() {
                ctx.request_repaint();
            }
        }
    }

    pub fn clear_infection(&self) {
        self.infection_found.store(false, Ordering::SeqCst);
        if let Ok(ctx_opt) = self.egui_ctx.lock() {
            if let Some(ctx) = ctx_opt.as_ref() {
                ctx.request_repaint();
            }
        }
    }

    pub fn add_scan(&self, path: String) -> Arc<AtomicBool> {
        let cancel_flag = Arc::new(AtomicBool::new(false));
        if let Ok(mut scans) = self.active_scans.lock() {
            scans.insert(
                path.clone(),
                ScanState {
                    current_file: "Starting...".to_string(),
                    cancel_flag: Arc::clone(&cancel_flag),
                },
            );
        }
        self.increment();
        cancel_flag
    }

    pub fn update_scan_status(&self, path: &str, current_file: String) {
        if let Ok(mut scans) = self.active_scans.lock() {
            if let Some(state) = scans.get_mut(path) {
                state.current_file = current_file;
            }
        }
        // Repaint to show update in GUI
        if let Ok(ctx_opt) = self.egui_ctx.lock() {
            if let Some(ctx) = ctx_opt.as_ref() {
                ctx.request_repaint();
            }
        }
    }

    pub fn cancel_scan(&self, path: &str) {
        if let Ok(scans) = self.active_scans.lock() {
            if let Some(state) = scans.get(path) {
                state.cancel_flag.store(true, Ordering::SeqCst);
            }
        }
    }

    pub fn remove_scan(&self, path: &str) {
        if let Ok(mut scans) = self.active_scans.lock() {
            scans.remove(path);
        }
        self.decrement();
    }

    pub fn add_history_entry(
        &self,
        path: String,
        status: ScanStatus,
        details: String,
        infected_files: Vec<String>,
    ) {
        if let Ok(mut history) = self.history.lock() {
            history.add_entry(path, status, details, infected_files);
        }
    }
}

pub struct ScanGuard {
    pub path: String,
    pub app_state: Arc<AppState>,
}

impl Drop for ScanGuard {
    fn drop(&mut self) {
        self.app_state.remove_scan(&self.path);
    }
}

use std::path::PathBuf;
use std::sync::mpsc;
use eframe;

mod app;
mod client;

use app::ConfigApp;
use app::Config;
use app::UiState;

use shared::Commands;

fn main() -> std::io::Result<()> {
    let config_path = PathBuf::from("photo-client-config.json");

    let (tx, rx) = mpsc::channel::<Commands>();

    let app = ConfigApp {
        config: Config::load_from_file(config_path.to_str().unwrap()),
        config_path,
        log_messages: Vec::new(),
        client_handle: None,
        stop_flag: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)),
        app_rx: rx,
        app_tx: tx,
        cli_tx: None,
        ui: UiState::default(),
    };

    let native_options = eframe::NativeOptions::default();
    let _ = eframe::run_native(
        "Photo Backup Service",
        native_options,
        Box::new(|_cc| Ok(Box::new(app))),
    );

    Ok(())
}
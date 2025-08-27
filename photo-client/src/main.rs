use std::path::PathBuf;
use std::sync::mpsc;
use eframe;

mod app;
mod client;
mod filestreamclient;

use app::ConfigApp;
use app::Config;
use app::UiState;

use crate::app::Commands;

fn main() -> std::io::Result<()> {

    let config_path = PathBuf::from("photo-client-config.json");
    let (tx, rx) = mpsc::channel::<Commands>();

    if std::path::Path::new("output.log").exists() {
        let file = std::fs::File::options().write(true).open("output.log")?;
        file.set_len(0)?;
    }

    let app = ConfigApp {
        config: Config::load_from_file(config_path.to_str().unwrap()),
        config_path,
        log_file: std::fs::File::create("output.log")?,
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
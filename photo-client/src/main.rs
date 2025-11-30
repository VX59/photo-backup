use std::{path::{Path,PathBuf}, sync::{mpsc, Arc, atomic}, fs, collections::HashMap};
use eframe;
use app::{App, ClientConfig, UiState, Commands};

mod app;
mod client;
mod filestreamclient;

fn main() -> std::io::Result<()> {

    let config_path = "photo-client-config.json";
    let config:ClientConfig;
    if Path::new(config_path).exists() {
        config = ClientConfig::load_from_file(config_path);
    } else {
        config = ClientConfig { server_address: "".to_string(), server_storage_directory: "".to_string(), repo_config: HashMap::new()};
    }
    let (tx, rx) = mpsc::channel::<Commands>();

    if Path::new("output.log").exists() {
        let file = std::fs::File::options().write(true).open("output.log")?;
        file.set_len(0)?;
    }

    config.save_to_file(config_path);

    let app = App {
        config: config,
        config_path: PathBuf::from(config_path),
        log_file: fs::File::create("output.log")?,
        client_handle: None,
        stop_flag: Arc::new(atomic::AtomicBool::new(true)),
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
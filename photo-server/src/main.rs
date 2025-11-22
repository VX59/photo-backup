use std::path::PathBuf;

use hostname::get;
use request_handler::Config;
use server::PhotoServer;
mod server;
mod filestreamserver;
mod request_handler;

fn main() {

    // load the config
    let config_path = PathBuf::from("photo-server-config.json");

    if !config_path.exists() {
        if let Err(e) = std::fs::File::create(&config_path) {
            println!("Unable to create the config file. {}", e);
        }
    }

    let mut config = Config::load_from_file("photo-server-config.json");
    config.config_path = "photo-server-config.json".to_string();
    config.save_to_file(&config.config_path);

    let name = get().unwrap_or_default().to_string_lossy().to_string(); // gets the servers hostname dynamically
    let mut photo_server = PhotoServer::new(
        name.clone(),
        format!("{}:{}", name, "8080"),
    );

    if let Err(e) = photo_server.start() {
        println!("Photo server encountered an error. {}", e);
    }
}
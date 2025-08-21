use std::path::PathBuf;
use hostname::get;
use server::PhotoServer;
use server::Config;

mod server;
mod filestreamserver;

fn main() {

    // load the config
    let config_path = PathBuf::from("photo-server-config.json");

    if !config_path.exists() {
        if let Err(e) = std::fs::File::create(&config_path) {
            println!("Unable to create the config file. {}", e);
        }
    }

    let name = get().unwrap_or_default().to_string_lossy().to_string(); // gets the servers hostname dynamically
    let mut photo_server = PhotoServer::new(
        name.clone(),
        format!("{}:{}", name, "8080"),
        "./storage-server".to_string(),
        Config::load_from_file(config_path.to_str().unwrap())
    );

    if let Err(e) = photo_server.start() {
        println!("Photo server encountered an error. {}", e);
    }
}
use std::{env, path::PathBuf};
use hostname::get;
use server::PhotoServer;
use request_handler::request_handler_utils::ServerConfig;
mod server;
mod filestreamserver;

mod request_handler;

fn main() {
    let args:Vec<String> = env::args().collect();
    let port:&str = if args.len() > 1 {
         &args[1]
    } else {
        "8080"
    };

    let config_name = "photo-server-config.json";
    let config_path = PathBuf::from(config_name);
    if !config_path.exists() {
        if let Err(e) = std::fs::File::create(&config_path) {
            println!("Unable to create the config file. {}", e);
        }
    }

    let mut config = ServerConfig::load_from_file(config_name);
    config.config_path = config_name.to_string();
    config.save_to_file(&config.config_path);

    let hostname = get().unwrap_or_default().to_string_lossy().to_string();
    let mut photo_server = PhotoServer::new(
        hostname.clone(),
        format!("{}:{}", hostname, port),
    );

    if let Err(e) = photo_server.start() {
        println!("Photo server encountered an error. {}", e);
    }
}
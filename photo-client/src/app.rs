use serde::Deserialize;
use serde::Serialize;
use std::sync::mpsc::Receiver;
use std::path::PathBuf;
use std::sync::mpsc;
use std::io::Read;
use egui::{Checkbox, RichText};
pub struct ConfigApp {
    pub config: Config,
    pub config_path: PathBuf,
    pub log_messages: Vec<String>,
    pub client_handle: Option<std::thread::JoinHandle<()>>,
    pub stop_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub rx: Receiver<String>,
    pub tx: mpsc::Sender<String>
}

use crate::client::ImageClient;

impl Config {
    pub fn load_from_file(path: &str) -> Self {
        let config_content = std::fs::read_to_string(path)
            .unwrap_or_else(|_| {
                println!("Config file not found, using default configuration.");
                String::new()
            });
        serde_json::from_str(&config_content).unwrap_or_else(|_| {
            println!("Failed to parse config file, using default configuration.");
            Config::default()
        })
    }

    pub fn save_to_file(&self, path: &str) {
        if let Ok(config_content) = serde_json::to_string_pretty(self) {
            if let Err(e) = std::fs::write(path, config_content) {
                eprintln!("Failed to write config file: {}", e);

            }
        }   
    }
}

#[derive(Serialize,Deserialize, Default, Debug, Clone)]
pub struct Config {
    pub server_address: String,
    pub watch_directory: String,
    pub server_repo_path: String,
    pub recursive_backup: bool,
}

impl eframe::App for ConfigApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Photo Client Configuration");

            ui.label("Server Address:");
            ui.text_edit_singleline(&mut self.config.server_address);

            ui.label("Watch Directory:");
            ui.text_edit_singleline(&mut self.config.watch_directory);

            ui.separator();
            ui.heading("Photo Server Configuration");

            ui.label("Repository Path on Server:");
            ui.text_edit_singleline(&mut self.config.server_repo_path);

            if ui.button("Save Configuration").clicked() {
                self.tx.send("Saving configuration...".to_string()).unwrap();

                // check if the monitored path exists
                if !std::path::Path::new(&self.config.watch_directory).exists() {
                    self.tx.send("Watch directory is not a valid path.".to_string()).unwrap();
                }
                self.config.save_to_file(self.config_path.to_str().unwrap());
            }

            ui.separator();
            ui.heading("Photo Client Control");

            if ui.button("Start Photo Client").clicked() {

                self.tx.send("Starting the photo client...".to_string()).unwrap();
                self.stop_flag.store(false, std::sync::atomic::Ordering::Relaxed);
               
                let tx_clone = self.tx.clone();
                let stop_flag_clone = self.stop_flag.clone();

                self.client_handle = Some(std::thread::spawn(move || {
                    let mut client = ImageClient::new(tx_clone, stop_flag_clone);
                    client.run().expect("Photo client encountered an error");
                }));

            }
            
            ui.add(Checkbox::new(&mut self.config.recursive_backup, RichText::new("Enable Recursive Backup").italics()));

            if ui.button("Backup Now").clicked() {
                if self.stop_flag.load(std::sync::atomic::Ordering::Relaxed) == true{
                    self.tx.send("Photo Client is not running. Start the client before backing up.".to_string()).unwrap();
                    return;
                }


                let fmt = "%Y-%m-%d %H:%M:%S%.f %:z";
                let f = std::fs::File::open("./last_backup.txt");
                let last_backup_time = match f {
                    Ok(mut file) => {
                        let mut contents = String::new();
                        if let Some(_) = file.read_to_string(&mut contents).ok() {
                            match chrono::DateTime::parse_from_str(contents.trim(), fmt) {
                                Ok(dt) => Some(dt.with_timezone(&chrono::Local)),
                                Err(_) => {
                                    self.tx.send("Failed to parse last backup time. Using (NOW)".to_string()).unwrap();
                                    None
                                }
                            }
                        } else {
                            self.tx.send("Failed to read last backup time. Using (NOW)".to_string()).unwrap();
                            None
                        }
                    }
                    Err(_) => {
                        self.tx.send("No previous backup time found".to_string()).unwrap();
                        chrono::Local::now().checked_sub_signed(chrono::Duration::seconds(1))
                    }
                };

                self.tx.send(format!("backing up all files modified since: {}", 
                    match last_backup_time {
                        Some(t) => t.format("%Y-%m-%d %H:%M:%S").to_string(),
                        None => "(NOW)".to_string()
                    }
                )).unwrap();
            }

            if ui.button("Stop Photo Client").clicked() {
                if self.stop_flag.load(std::sync::atomic::Ordering::Relaxed) != false {
                    self.tx.send("Photo Client is already stopped or never started.".to_string()).unwrap();
                    return;
                }

                self.stop_flag.store(true, std::sync::atomic::Ordering::Relaxed);

                let _ = self.tx.send("Stopping Photo Client...".to_string());

                if let Some(handle) = self.client_handle.take() {
                    let _ = handle.join();
                }
            }
            
            if ui.button("Clear Log").clicked() {
                self.log_messages.clear();
            }

            ui.separator();
            ui.heading("Log Messages:");
            for msg in &self.log_messages {
                ui.label(msg);
            }
        });

        while let Ok(msg) = self.rx.try_recv() {
            self.log_messages.push(msg);
        }
        ctx.request_repaint();

    }
}
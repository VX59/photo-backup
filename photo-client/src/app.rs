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
    pub app_rx: Receiver<Commands>,
    pub app_tx: mpsc::Sender<Commands>,
    pub cli_tx: Option<mpsc::Sender<Commands>>,
    pub ui: UiState,
}

pub enum Commands {
    Log(String),
    CreateRepo(String),
    PostRepos(Vec<String>),
    StartStream(String),
}

pub struct UiState {
    pub show_create_ui: bool,
    pub show_repos: bool,
    pub available_repos: Vec<String>,
    pub new_repo_name: String,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            show_create_ui: false,
            show_repos: false,
            available_repos: vec![],
            new_repo_name: String::new(),
        }
    }
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
    pub server_storage_directory: String,
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

            ui.label("Global storage path on Server:");
            ui.text_edit_singleline(&mut self.config.server_storage_directory);

            if ui.button("Save Configuration").clicked() {
                self.app_tx.send(Commands::Log("Saving configuration...".to_string())).unwrap();

                // check if the monitored path exists
                if !std::path::Path::new(&self.config.watch_directory).exists() {
                    self.app_tx.send(Commands::Log("Watch directory is not a valid path.".to_string())).unwrap();
                }
                self.config.save_to_file(self.config_path.to_str().unwrap());
            }

            ui.separator();
            ui.heading("Photo Client Control");

            if ui.button("New Repository").clicked() {
                self.ui.show_create_ui = !self.ui.show_create_ui;
                if !self.ui.show_create_ui {
                    self.ui.new_repo_name.clear();
                }
            }

            if self.ui.show_create_ui {
                ui.horizontal(|ui| {
                    ui.text_edit_singleline(&mut self.ui.new_repo_name);

                    if ui.button("Create").clicked() {

                        if let Some(cli_tx) = &self.cli_tx {
                            self.app_tx.send(Commands::Log(format!("Creating repository {}", self.ui.new_repo_name).to_string())).unwrap();
                            cli_tx.send(Commands::CreateRepo(self.ui.new_repo_name.to_string())).unwrap();
                            
                        } else {
                            self.app_tx.send(Commands::Log("The client isn't running".to_string())).unwrap();
                        }
                        
                        self.ui.new_repo_name.clear();
                        self.ui.show_create_ui = false;
                    }
                });
            }

            // opens the command channel.. once a repository is open open a seperate streaming channel
            if ui.button("Connect to server").clicked() {
                if self.stop_flag.load(std::sync::atomic::Ordering::Relaxed) {
                    self.app_tx.send(Commands::Log("Launching a Photo Client command channel...".to_string())).unwrap();
                    self.stop_flag.store(false, std::sync::atomic::Ordering::Relaxed);
                
                    let log_tx_clone = self.app_tx.clone();
                    let stop_flag_clone = self.stop_flag.clone();

                    let (cmd_tx, cmd_rx) = mpsc::channel::<Commands>();
                    self.cli_tx = Some(cmd_tx.clone());

                    self.client_handle = Some(std::thread::spawn(move || {
                        let mut client = ImageClient::new(log_tx_clone, cmd_rx, stop_flag_clone);
                        if let Err(e) = client.connect() {
                           client.app_tx.send(Commands::Log(format!("{}",e).to_string())).unwrap();
                        }
                    }));
                } else {
                    self.app_tx.send(Commands::Log("The client is already running".to_string())).unwrap();
                }  
            }
            
            if self.ui.show_repos {
                ui.horizontal(|ui| {
                    for repo in self.ui.available_repos.clone() {
                        if ui.button(&repo).clicked() {
                            if let Some(cli_tx) = &self.cli_tx {
                                cli_tx.send(Commands::StartStream(repo.to_string())).unwrap();
                            } else {
                                self.app_tx.send(Commands::Log("The client isn't running".to_string())).unwrap();
                            }
                        }
                    }
                });
            }

            ui.add(Checkbox::new(&mut self.config.recursive_backup, RichText::new("Enable Recursive Backup").italics()));

            if ui.button("Backup Now").clicked() {
                if self.stop_flag.load(std::sync::atomic::Ordering::Relaxed) == true{
                    self.app_tx.send(Commands::Log("Photo Client is not running. Start the client before backing up.".to_string())).unwrap();
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
                                    self.app_tx.send(Commands::Log("Failed to parse last backup time. Using (NOW)".to_string())).unwrap();
                                    None
                                }
                            }
                        } else {
                            self.app_tx.send(Commands::Log("Failed to read last backup time. Using (NOW)".to_string())).unwrap();
                            None
                        }
                    }
                    Err(_) => {
                        self.app_tx.send(Commands::Log("No previous backup time found".to_string())).unwrap();
                        chrono::Local::now().checked_sub_signed(chrono::Duration::seconds(1))
                    }
                };

                self.app_tx.send(Commands::Log(format!("backing up all files modified since: {}", 
                    match last_backup_time {
                        Some(t) => t.format("%Y-%m-%d %H:%M:%S").to_string(),
                        None => "(NOW)".to_string()
                    }
                ))).unwrap();
            }

            if ui.button("Stop Photo Client").clicked() {
                if self.stop_flag.load(std::sync::atomic::Ordering::Relaxed) != false {
                    self.app_tx.send(Commands::Log("Photo Client is already stopped or never started.".to_string())).unwrap();
                    return;
                }

                self.stop_flag.store(true, std::sync::atomic::Ordering::Relaxed);

                let _ = self.app_tx.send(Commands::Log("Stopping Photo Client...".to_string()));

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

        while let Ok(msg) = self.app_rx.try_recv() {
            match msg {
                Commands::Log(msg) => {
                    self.log_messages.push(msg);
                }
                Commands::PostRepos(repos) => {
                    self.ui.show_repos = true;
                    self.ui.available_repos = repos;
                }
                _ => {},
            }
        }
        ctx.request_repaint();

    }
}
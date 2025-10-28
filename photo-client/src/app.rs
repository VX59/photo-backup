use egui::ahash::HashMap;
use serde::Deserialize;
use serde::Serialize;
use std::sync::mpsc::Receiver;
use std::path::PathBuf;
use std::sync::mpsc;
use std::io::{Read,Write};
use egui::{Color32, RichText, Frame, Checkbox};
pub struct ConfigApp {
    pub config: Config,
    pub config_path: PathBuf,
    pub log_file: std::fs::File,
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
    UpdateRepoStatus((String, ConnectionStatus)),
    StartStream(String),
    DisconnectStream(String),
    UpdateConnectionStatus(ConnectionStatus),
    RemoveRepository(String),
}

#[derive(PartialEq)]
pub enum ConnectionStatus {
    Connected,
    Connecting,
    Disconnected,
    Disconnecting
}

impl std::fmt::Display for ConnectionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConnectionStatus::Connected => write!(f, "Connected"),
            ConnectionStatus::Connecting => write!(f, "Connecting"),
            ConnectionStatus::Disconnected => write!(f, "Disconnected"),
            ConnectionStatus::Disconnecting => write!(f, "Disconnected"),

        }
    }
}

pub struct UiState {
    pub show_create_ui: bool,
    pub new_repo_name: String,
    pub selected_repo: Option<usize>,
    pub connection_status: ConnectionStatus,
    pub repo_status: std::collections::HashMap<String, ConnectionStatus>,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            connection_status: ConnectionStatus::Disconnected,
            show_create_ui: false,
            new_repo_name: String::new(),
            selected_repo: None,
            repo_status: std::collections::HashMap::new(),
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

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
pub struct RepoConfig {
    pub auto_connect: bool,
    pub watch_directory: String,
}

#[derive(Serialize,Deserialize, Default, Debug, Clone)]
pub struct Config {
    pub server_address: String,
    pub server_storage_directory: String,
    pub repo_config: HashMap<String, RepoConfig>,
}

impl ConfigApp {
    fn connect_to_server (&mut self, ui:&mut egui::Ui) {
        if self.ui.connection_status == ConnectionStatus::Disconnected {
            if ui.button("Connect to server").clicked() {
                if self.config.server_address.is_empty() | self.config.server_storage_directory.is_empty() {
                    self.app_tx.send(Commands::Log("server address or storage directory not set".to_string())).unwrap();
                } else {
                    self.app_tx.send(Commands::Log("Saving configuration...".to_string())).unwrap();
                    self.config.save_to_file(self.config_path.to_str().unwrap());

                    if self.stop_flag.load(std::sync::atomic::Ordering::Relaxed) {
                        self.ui.connection_status = ConnectionStatus::Connecting;
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
            }
        }
    }

    fn disconnect_from_server(&mut self, ui:&mut egui::Ui) {
        if self.ui.connection_status == ConnectionStatus::Connected {
            if ui.button("Disconnect").clicked() {
                if self.stop_flag.load(std::sync::atomic::Ordering::Relaxed) != false {
                    self.app_tx.send(Commands::Log("Photo Client is already stopped or never started.".to_string())).unwrap();
                    return;
                }
                self.ui.connection_status = ConnectionStatus::Disconnecting;
                self.stop_flag.store(true, std::sync::atomic::Ordering::Relaxed);

                let _ = self.app_tx.send(Commands::Log("Stopping Photo Client...".to_string()));

                if let Some(handle) = self.client_handle.take() {
                    let _ = handle.join();
                }
            }
        }
    }

    fn connect_config(&mut self, ui:&mut egui::Ui) {
        ui.label("Server Address:");
        ui.text_edit_singleline(&mut self.config.server_address);
        ui.label("Global storage path on Server:");
        ui.text_edit_singleline(&mut self.config.server_storage_directory);
    }

    fn connect_menu(&mut self, ui:&mut egui::Ui) {
        let status_color = match self.ui.connection_status {
            ConnectionStatus::Connected => Color32::GREEN,
            ConnectionStatus::Disconnected => Color32::GRAY,
            ConnectionStatus::Connecting | ConnectionStatus::Disconnecting => Color32::ORANGE, 
        };

        ui.horizontal(|ui| {
            ui.heading("Photo Server Configuration");
            let badge_frame = Frame::default()
                .fill(Color32::TRANSPARENT)
                .stroke(egui::Stroke::new(2.0,status_color))
                .corner_radius(4);

            badge_frame.show(ui, |ui| {
                ui.label(
                    RichText::new(format!("{}",self.ui.connection_status))
                        .color(status_color)
                        .strong(),
                );
            });
            ui.with_layout(egui::Layout::left_to_right(egui::Align::TOP), |ui| {
                ui.vertical(|ui| {
                    self.disconnect_from_server(ui);
                    self.connect_to_server(ui);
                });
            });

        });
        self.connect_config(ui);
    }

    fn repository_list(&mut self, ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            ui.heading("Repositories");

            let repo_names:Vec<&String> = self.ui.repo_status.keys().collect();
            for (i,repo) in repo_names.iter().enumerate() {
                let selected = self.ui.selected_repo == Some(i);
                if ui.selectable_label(selected,*repo).clicked() {
                    /**/
                    self.ui.selected_repo = Some(i)
                }
            }
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
                            self.config.repo_config.entry(self.ui.new_repo_name.clone()).or_default();

                            cli_tx.send(Commands::CreateRepo(self.ui.new_repo_name.to_string())).unwrap();
                            
                        } else {
                            self.app_tx.send(Commands::Log("The client isn't running".to_string())).unwrap();
                        }
                        
                        self.ui.new_repo_name.clear();
                        self.ui.show_create_ui = false;
                    }
                });
            }
        });   
    }

    fn repository_controls(&mut self, ui:&mut egui::Ui) {
        ui.vertical(|ui| {
            if let Some(i) = self.ui.selected_repo {
                let repo_names: Vec<String> = self.ui.repo_status.keys().cloned().collect();
                let repo_name = repo_names[i].clone();
                
                let status_text = match self.ui.repo_status.get(&repo_name) {
                    Some(status) => format!("{}", status),
                    None => "Unknown".to_string(),
                };

                let status_color = match self.ui.repo_status.get(&repo_name) {
                    Some(ConnectionStatus::Connected) => Color32::GREEN,
                    Some(ConnectionStatus::Disconnected) => Color32::GRAY,
                    Some(_) => Color32::ORANGE,
                    None => Color32::LIGHT_GRAY,
                };

                ui.horizontal(|ui| {
                    ui.heading(repo_name.clone());

                    Frame::default()
                        .fill(Color32::TRANSPARENT)
                        .stroke(egui::Stroke::new(2.0, status_color))
                        .corner_radius(4.0) // replaces rounding()
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(status_text)
                                    .color(status_color)
                                    .strong(),
                            );
                        });
                });
                
                ui.label("Watch Directory:");

                let repo_config = self.config.repo_config.entry(repo_name.clone()).or_default();

                ui.text_edit_singleline(&mut repo_config.watch_directory);
                
                if !repo_config.watch_directory.is_empty() && std::path::Path::new(&repo_config.watch_directory).exists() == true {
                    if self.ui.repo_status.get(&repo_name.clone()) == Some(&ConnectionStatus::Disconnected) {
                        if ui.button("Connect").clicked() {
                            self.app_tx.send(Commands::Log("Saving configuration...".to_string())).unwrap();
                            self.config.save_to_file(self.config_path.to_str().unwrap());

                            self.ui.repo_status.insert(repo_name.to_string(), ConnectionStatus::Connecting);
                            if let Some(cli_tx) = &self.cli_tx {
                                cli_tx.send(Commands::StartStream(repo_name.to_string())).unwrap();
                            }
                        }
                    }
                }
                
                ui.add(Checkbox::new(&mut self.config.repo_config.entry(repo_name.clone()).or_default().auto_connect, RichText::new("Enable auto-connect").italics()));

                if self.ui.repo_status.get(&repo_name.clone()) == Some(&ConnectionStatus::Connected) {
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

                    if ui.button("Disconnect").clicked() {

                        self.ui.repo_status.insert(repo_name.clone(), ConnectionStatus::Disconnecting);
                        if let Some(cli_tx) = &self.cli_tx {
                            cli_tx.send(Commands::DisconnectStream(repo_name.to_string())).unwrap();
                        }
                    }
                    
                    if ui.button("Remove Repository").clicked() {
                        self.ui.repo_status.insert(repo_name.clone(), ConnectionStatus::Disconnecting);
                        if let Some(cli_tx) = &self.cli_tx {
                            cli_tx.send(Commands::RemoveRepository(repo_name.to_string())).unwrap();
                        }
                    }
                }                            
            } else {
                ui.label("Select a repository to see more details");
            }
        });
    }

    fn repository_menu(&mut self, ui: &mut egui::Ui) {
        if self.ui.connection_status == ConnectionStatus::Connected {
            ui.separator();
            ui.horizontal(|ui| {
                self.repository_list(ui);
                ui.separator();
                self.repository_controls(ui);
            });
        }
    }

    fn file_explorer(&mut self, ui: &mut egui::Ui, path:&mut String) {
      if self.ui.connection_status == ConnectionStatus::Connected {
          ui.vertical(|ui| {
            ui.label("File Explorer");

          });
      }  
    }

    fn client_command_receiver(&mut self) {
        while let Ok(msg) = self.app_rx.try_recv() {
            match msg {
                Commands::Log(msg) => {
                    writeln!(self.log_file, "{}", msg).ok();
                }
                Commands::PostRepos(repos) => {       
                    self.ui.selected_repo = None;
                    self.ui.repo_status.clear();             
                    for repo in repos {
                        self.ui.repo_status.insert(repo, ConnectionStatus::Disconnected);
                    }
                }

                Commands::UpdateRepoStatus((repo,status)) => {
                    self.ui.repo_status.insert(repo, status);
                }

                Commands::UpdateConnectionStatus(status) => self.ui.connection_status = status,

                Commands::RemoveRepository(repo) => {
                    self.config.repo_config.remove(&repo);
                    self.ui.repo_status.remove(&repo);
                    self.ui.selected_repo = None;
                    self.config.save_to_file(self.config_path.to_str().unwrap());
                }
                _ => {},
            }
        }
    }
}

impl eframe::App for ConfigApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            self.connect_menu(ui);       
            self.repository_menu(ui);
        });
        
        self.client_command_receiver();
        ctx.request_repaint();
    }
}
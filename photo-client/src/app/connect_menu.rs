use egui::{Color32, RichText, Frame};
use super::{Commands, ConnectionStatus, App};
use std::sync::mpsc;
use crate::client::Client;

impl App {
    fn connect_to_server (&mut self, ui:&mut egui::Ui) {
        if self.ui.connection_status == ConnectionStatus::Disconnected {
            if ui.button("Connect to server").clicked() {
                self.ui.file_explorer_path.clear();
                self.ui.subdir_contents = None;
                self.ui.tree = None;
                if self.config.server_address.is_empty() | self.config.server_storage_directory.is_empty() {
                    let message = "server address or storage directory not set".to_string();
                    self.app_tx.send(Commands::Log(message.clone())).unwrap();
                    self.app_tx.send(Commands::Notify(message.clone())).unwrap();
                } else {
                    if self.stop_flag.load(std::sync::atomic::Ordering::Relaxed) {
                        self.ui.connection_status = ConnectionStatus::Connecting;
                        let message = "Launching a Photo Client command channel...".to_string();
                        self.app_tx.send(Commands::Log(message.clone())).unwrap();
                        self.app_tx.send(Commands::Notify(message.clone())).unwrap();
                        self.stop_flag.store(false, std::sync::atomic::Ordering::Relaxed);
                    
                        let log_tx_clone = self.app_tx.clone();
                        let stop_flag_clone = self.stop_flag.clone();

                        let (cmd_tx, cmd_rx) = mpsc::channel::<Commands>();
                        self.cli_tx = Some(cmd_tx.clone());

                        self.client_handle = Some(std::thread::spawn(move || {
                            let mut client = Client::new(log_tx_clone, cmd_rx, stop_flag_clone);
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
                self.ui.connection_status = ConnectionStatus::Disconnecting;
                self.stop_flag.store(true, std::sync::atomic::Ordering::Relaxed);

                let message = "Stopping Client...".to_string();
                let _ = self.app_tx.send(Commands::Log(message.clone()));
                let _ = self.app_tx.send(Commands::Notify(message.clone()));

                if let Some(handle) = self.client_handle.take() {
                    let _ = handle.join();
                }
            }
        }
    }

    fn connect_config(&mut self, ui:&mut egui::Ui) {
        ui.label("Server Address:");
        ui.text_edit_singleline(&mut self.config.server_address);
    }

    pub fn connect_menu(&mut self, ui:&mut egui::Ui) {
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

        if self.ui.connection_status == ConnectionStatus::Connected {
            ui.label("Global storage path");
            ui.set_width(ui.available_width());

            ui.horizontal(|ui| {
                ui.text_edit_singleline(&mut self.config.server_storage_directory);
                if ui.button("Save global storage path").clicked() {
                    if let Some(cli_tx) = &self.cli_tx {
                        let storage_path = self.config.server_storage_directory.clone();
                        cli_tx.send(Commands::SetStoragePath(storage_path)).unwrap()
                    }
                }
                if ui.button("Save configuration").clicked() {
                    let message = "Saving configuration...".to_string();
                    self.app_tx.send(Commands::Log(message.clone())).unwrap();
                    self.app_tx.send(Commands::Notify(message.clone())).unwrap();

                    self.config.save_to_file(self.config_path.to_str().unwrap());
                }
                if let Some(notification) = &mut self.ui.notification {
                    ui.label(notification.to_string());
                }
            });
        }
    }
}
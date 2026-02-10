use super::App;
use egui::{Checkbox, Color32, Frame, RichText, ScrollArea};
use super::{Commands, ConnectionStatus};

impl App {
    fn repository_list(&mut self, ui: &mut egui::Ui) {
 
        ui.vertical(|ui| {
            ui.heading("Repositories");

            let repo_names:Vec<&String> = self.ui.repo_status.keys().collect();
            for (i,repo) in repo_names.iter().enumerate() {
                let selected = self.ui.selected_repo == Some(i);
                if ui.selectable_label(selected,*repo).clicked() {
                    self.ui.selected_repo = Some(i);
                    let repo_name = repo.to_string();
                    self.ui.file_explorer_path.clear();
                    self.ui.file_explorer_path.push(repo_name.clone());
                    if let Some(cli_tx) = &self.cli_tx {
                        cli_tx.send(Commands::GetRepoTree(repo_name.clone())).unwrap();
                    }
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
                
                if !repo_config.watch_directory.is_empty() && std::path::Path::new(&repo_config.watch_directory).exists() {
                    if self.ui.repo_status.get(&repo_name.clone()) == Some(&ConnectionStatus::Disconnected) {
                        if ui.button("Connect").clicked() {
                            self.ui.repo_status.insert(repo_name.to_string(), ConnectionStatus::Connecting);
                            if let Some(cli_tx) = &self.cli_tx {
                                cli_tx.send(Commands::StartEventListener(repo_name.to_string(), repo_config.watch_directory.to_string())).unwrap();
                            }
                        }
                    }
                }
                
                ui.add(Checkbox::new(&mut self.config.repo_config.entry(repo_name.clone()).or_default().auto_connect, RichText::new("Enable auto-connect").italics()));
                ui.add(Checkbox::new(&mut self.config.repo_config.entry(repo_name.clone()).or_default().track_modifications, RichText::new("Track file modifications").italics()));

                if self.ui.repo_status.get(&repo_name) == Some(&ConnectionStatus::Connected) {

                    if ui.button("Discover Untracked").clicked() {
                        if let Some(cli_tx) = &self.cli_tx {
                            cli_tx.send(Commands::DiscoverUntracked(repo_name.to_string())).unwrap();
                        }
                    }

                    if ui.button("Disconnect").clicked() {

                        self.ui.repo_status.insert(repo_name.clone(), ConnectionStatus::Disconnecting);
                        if let Some(cli_tx) = &self.cli_tx {
                            cli_tx.send(Commands::DisconnectStream(repo_name.to_string())).unwrap();
                            self.ui.file_explorer_path.clear();
                            self.ui.subdir_contents = None;
                            self.ui.tree = None;
                            self.ui.selected_repo = None;
                        }
                    }
                }
                if ui.button("Remove repository").clicked() {
                    self.ui.show_remove_ui = !self.ui.show_remove_ui;
                    if !self.ui.show_remove_ui {
                        self.ui.remove_repo_name.clear();
                    }
                }
                if self.ui.show_remove_ui {
                    ui.horizontal(|ui| {
                        let response = ui.text_edit_singleline(&mut self.ui.remove_repo_name);
                        
                        if self.ui.remove_repo_name.is_empty() && !response.has_focus() {
                            let painter = ui.painter();
                            painter.text(
                                response.rect.left_top() + egui::vec2(4.0, 2.0), // small padding inside box
                                egui::Align2::LEFT_TOP,
                                "Enter repo name to remove...",
                                egui::TextStyle::Body.resolve(ui.style()),
                                ui.visuals().weak_text_color(), // faded color
                            );
                        }

                        if self.ui.remove_repo_name == repo_name.clone() {
                            if ui.button("permanently remove").clicked() {
                                self.ui.repo_status.insert(repo_name.clone(), ConnectionStatus::Disconnecting);
                                if let Some(cli_tx) = &self.cli_tx {
                                    cli_tx.send(Commands::RemoveRepository(repo_name.to_string())).unwrap();
                                }
                                self.ui.remove_repo_name.clear();
                                self.ui.show_remove_ui = false;
                            }
                        }
                    });
                }
            } else {
                ui.label("Select a repository to see more details");
            }
        });
    }

    fn file_explorer(&mut self, ui: &mut egui::Ui) {
      if self.ui.connection_status == ConnectionStatus::Connected {
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.heading("File Explorer");
                if let Some(tree) = &self.ui.tree {
                    ui.label(format!("{} files, {} subdirs", tree.history.len(), tree.content.len()));
                }
            });
            ui.horizontal(|ui| {
                for i in 0..self.ui.file_explorer_path.len() {
                    let subdir = self.ui.file_explorer_path[i].clone();
                    if ui.button(format!("/{}", subdir)).clicked() {
                        self.ui.file_explorer_path.truncate(i+1);
                        self.app_tx.send(Commands::GetSubDir(subdir.clone())).unwrap();
                        break;
                    };
                }
            });
            if let Some(contents) = &self.ui.subdir_contents {
                ScrollArea::vertical()
                .auto_shrink([false;2])
                .show(ui, |ui| {
                    for entry in contents {
                        if entry.is_directory{
                            if ui.button(entry.name.to_string()).clicked() {
                                self.ui.file_explorer_path.push(entry.name.clone());
                                self.app_tx.send(Commands::GetSubDir(entry.name.clone())).unwrap();
                            }
                        } else {
                            ui.label(entry.name.clone());
                        }
                    }
                });
            } else {
                ui.label("Directory is Empty");
            }
          });
      }  
    }
    
    pub fn repository_menu(&mut self, ui: &mut egui::Ui) {
        if self.ui.connection_status == ConnectionStatus::Connected {
            ui.separator();
            ui.horizontal(|ui| {
                self.repository_list(ui);
                ui.separator();
                self.repository_controls(ui);
            });
            ui.separator();
            self.file_explorer(ui);
        }
    }
}
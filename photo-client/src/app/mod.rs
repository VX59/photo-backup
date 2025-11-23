pub mod connect_menu;
pub mod repository_menu;
pub mod app_utils;

use std::{sync::mpsc, path::PathBuf, io::Write};
pub use app_utils::{ConnectionStatus, RepoConfig, Commands, ClientConfig, UiState, FileSystemEntry};

pub struct App {
    pub config: ClientConfig,
    pub config_path: PathBuf,
    pub log_file: std::fs::File,
    pub client_handle: Option<std::thread::JoinHandle<()>>,
    pub stop_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub app_rx: mpsc::Receiver<Commands>,
    pub app_tx: mpsc::Sender<Commands>,
    pub cli_tx: Option<mpsc::Sender<Commands>>,
    pub ui: UiState,
}

impl App {
    fn client_command_receiver(&mut self) {
        while let Ok(msg) = self.app_rx.try_recv() {
            match msg {
                Commands::Log(msg) => {
                    writeln!(self.log_file, "{}", msg).ok();
                }

                Commands::GetSubDir(subdir_name) => {
                    self.ui.subdir_contents = None;
                    if let Some(tree) = &self.ui.tree {
                        if let Some(contents) = tree.content.get(&subdir_name) {
                            let mut fs_entries:Vec<FileSystemEntry> = Vec::new();
                            for entry in contents {
                                let fs_entry = FileSystemEntry {
                                    name: entry.clone(),
                                    is_directory: tree.content.contains_key(entry),
                                };
                                fs_entries.push(fs_entry);
                            }
                            self.ui.subdir_contents = Some(fs_entries);
                        }
                    }
                }
                Commands::PostRepoTree(tree, repo_name) => {
                    self.ui.tree = Some(tree);
                    self.app_tx.send(Commands::GetSubDir(repo_name)).ok(); // maybe issue well see
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
                    if let Some(tree) = self.ui.tree.take() {
                        let tree_path = tree.path.clone();
                        std::fs::remove_file(tree_path).ok();
                        self.ui.tree = None;
                        self.ui.subdir_contents = None;
                        self.ui.file_explorer_path.clear();
                    }

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

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            self.connect_menu(ui);       
            self.repository_menu(ui);
        });
        
        self.client_command_receiver();
        ctx.request_repaint();
    }
}
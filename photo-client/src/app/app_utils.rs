use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use shared::Tree;

pub enum Commands {
    Log(String),
    CreateRepo(String),
    SetStoragePath(String),
    PostRepos(Vec<String>),
    UpdateRepoStatus((String, ConnectionStatus)),
    StartStream(String, String),
    DisconnectStream(String),
    UpdateConnectionStatus(ConnectionStatus),
    RemoveRepository(String),
    GetRepoTree(String),
    PostRepoTree(Tree, String),
    GetSubDir(String),
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
pub struct FileSystemEntry {
    pub name: String,
    pub is_directory: bool,
}
pub struct UiState {
    pub show_create_ui: bool,
    pub new_repo_name: String,
    pub selected_repo: Option<usize>,
    pub connection_status: ConnectionStatus,
    pub repo_status: std::collections::HashMap<String, ConnectionStatus>,
    pub file_explorer_path:Vec<String>,
    pub subdir_contents:Option<Vec<FileSystemEntry>>,
    pub tree: Option<Tree>,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            connection_status: ConnectionStatus::Disconnected,
            show_create_ui: false,
            new_repo_name: String::new(),
            selected_repo: None,
            repo_status: std::collections::HashMap::new(),
            file_explorer_path: Vec::new(),
            subdir_contents: None,
            tree: None,
        }
    }
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
pub struct RepoConfig {
    pub auto_connect: bool,
    pub watch_directory: String,
}

#[derive(Serialize,Deserialize, Default, Debug, Clone)]
pub struct ClientConfig {
    pub server_address: String,
    pub server_storage_directory: String,
    pub repo_config: HashMap<String, RepoConfig>,
}

impl ClientConfig {
    pub fn load_from_file(path: &str) -> Self {
        let config_content = std::fs::read_to_string(path)
            .unwrap_or_else(|_| {
                println!("Config file not found, using default configuration.");
                String::new()
            });
        serde_json::from_str(&config_content).unwrap_or_else(|_| {
            println!("Failed to parse config file, using default configuration.");
            ClientConfig::default()
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
use serde::{Deserialize, Serialize};
use serde_json;

#[derive(Serialize,Deserialize, Default, Debug, Clone)]
pub struct ServerConfig {
    pub storage_directory:String,
    pub repo_list: Vec<String>,
    pub config_path: String,
}

impl ServerConfig {
    pub fn load_from_file(path: &str) -> Self {
        let config_content = std::fs::read_to_string(path)
        .unwrap_or_else(|_| {
            println!("Config file not found, using default configuration.");
            String::new()
        });
        serde_json::from_str(&config_content).unwrap_or_else(|_| {
            println!("Failed to parse config file, using default configuration.");
            ServerConfig::default()
        })
    }

    pub fn save_to_file(&self, path: &str) {
        if let Ok(config_content) = serde_json::to_string_pretty(self) {
            if let Err(e) = std::fs::write(path, config_content) {
                eprintln!("Failed to write config file: {}", e);

            }
        }   
    }
    pub fn remove_repo(&mut self, repo:String) {
        if self.repo_list.contains(&repo) {
            self.repo_list.retain(|r| r != &repo);
            self.save_to_file(&self.config_path);
        } else {
            eprintln!("Repo does not exist in config.");
        }
    }
    
    pub fn add_repo(&mut self, repo:String) {
        if !self.repo_list.contains(&repo) {
            self.repo_list.push(repo);
            self.save_to_file(&self.config_path);
        } else {
            eprintln!("Repo already exists in config.");
        }
    }
}
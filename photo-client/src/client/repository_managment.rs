use super::Client;
use std::collections::HashMap;
use shared::{send_request,RequestTypes,ResponseCodes,Tree,
            read_response, Request, Log};
use crate::app::{Commands, ConnectionStatus, RepoConfig};

impl Client {
    pub fn create_repository(&mut self, repo_name:String) -> anyhow::Result<()> {
        if let Some(stream) = self.command_stream.as_mut() {
            let request = Request {
                request_type: RequestTypes::CreateRepo,
                body: repo_name.as_bytes().to_vec(),
            };

            send_request(request, stream)?;

            let response = read_response(stream)?;
            self.log_response(&response)?;

            if response.status_code == ResponseCodes::OK {
                self.get_repositories()?;
                let tree:Tree = Tree {
                    version: 0,
                    content: HashMap::new(),
                    history: HashMap::new(),
                    path: ("trees".to_string() + "/" + &repo_name + ".tree").to_string(),
                    name: repo_name.clone(),
                };
                self.trees.insert(repo_name.clone(), tree.clone());
                Tree::save_to_file(&tree,&tree.path);

            }
        }

        let repo_config = RepoConfig::default();

        self.config.repo_config.insert(repo_name, repo_config);
        self.config.save_to_file("photo-client-config.json");
        Ok(())
    }

    pub fn get_repositories(&mut self) -> anyhow::Result<()> { 
        let request = Request {
            request_type: RequestTypes::GetRepos,
            body: vec![0u8,0],
        };

        if let Some(stream) = self.command_stream.as_mut() {
            send_request(request, stream)?;
            let response = read_response(stream)?;
            self.log_response(&response)?;

            if response.status_code == ResponseCodes::OK {
                // send the repo list to the app
                let available_repositories:Vec<String> = serde_json::from_slice(&response.body)?;
                self.app_tx.send(Commands::PostRepos(available_repositories))?;

                for (repo_name, repo_config) in self.config.repo_config.clone() {
                    if repo_config.auto_connect & self.config.repo_config.contains_key(&repo_name){
                        
                        // request a file streaming channel - the server will open another port
                        let stop_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                        let file_streaming_client_handle = self.initiate_file_streaming_client(repo_name.to_string(), repo_config.watch_directory.to_string(), stop_flag.clone())?;

                        self.repo_threads.insert(repo_name.clone(), (file_streaming_client_handle, stop_flag));
                    }
                    let tree_path = ("trees".to_string() + "/" + &repo_name + ".tree").to_string();
                    let tree:Tree = Tree::load_from_file(&tree_path);
                    self.trees.insert(repo_name, tree);
                }
            } else {
                self.app_tx.send(Commands::PostRepos(Vec::new()))?;
            }

        } else {
            self.app_tx.send(Commands::Log("Failed to get repositories, client not connected.".to_string()))?;
        }
        Ok(())
    }

    pub fn disconnect_repository(&mut self, repo:&String) -> anyhow::Result<()>{
        match self.repo_threads.remove(repo) {
            Some((handle,stop_flag)) => {
                stop_flag.store(true, std::sync::atomic::Ordering::Relaxed);
                self.app_tx.send(Commands::Log(format!("{} client thread stop flag set", repo).to_string()))?;

                if let Err(_e) = handle.join() {
                    self.app_tx.send(Commands::Log(format!("{} client thread failed to join", repo).to_string()))?;
                    return Err(anyhow::anyhow!(format!("{} client thread failed to join", repo)));
                }
            },
            None => {
                self.app_tx.send(Commands::Log(format!("Not connected to {}", repo).to_string()))?;
            }
        }

        if let Some(stream) = self.command_stream.as_mut() {
            let request = Request {
                request_type:RequestTypes::DisconnectStream,
                body: repo.as_bytes().to_vec(),
            };

            send_request(request, stream)?;

            let response = read_response(stream)?;
            self.log_response(&response)?;

            let new_status = match response.status_code {
                ResponseCodes::OK => ConnectionStatus::Disconnected,
                _ => ConnectionStatus::Connected,
            };

            self.app_tx.send(Commands::UpdateRepoStatus((repo.clone(),new_status)))?;
            
        }
        Ok(())
    }

    pub fn remove_repository(&mut self, repo_name:&String) -> anyhow::Result<()> {
        if let Some(stream) = self.command_stream.as_mut() {
            let request = Request {
                request_type: RequestTypes::RemoveRepository,
                body: repo_name.clone().as_bytes().to_vec(),
            };

            send_request(request, stream)?;
            let response = read_response(stream)?;
            self.log_response(&response)?;
        
            if response.status_code == ResponseCodes::OK {
                self.config.repo_config.remove(repo_name);
                self.trees.remove(repo_name);
                self.config.save_to_file("./photo-client-config.json");
                self.app_tx.send(Commands::RemoveRepository(repo_name.to_string()))?;
            }            
        }
        Ok(())
    }
}
use super::Client;
use std::{collections::HashMap, sync::{Arc, atomic}};
use shared::{send_request,RequestTypes,ResponseCodes,Tree,
            read_response, Request, Log, Notify};
use crate::app::{Commands, ConnectionStatus, RepoConfig};
use serde_json::json;

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
            self.notify_app(&response)?;

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
            self.notify_app(&response)?;

            if response.status_code == ResponseCodes::OK {
                // send the repo list to the app
                let available_repositories:Vec<String> = serde_json::from_slice(&response.body)?;
                self.app_tx.send(Commands::PostRepos(available_repositories))?;

                for (repo_name, repo_config) in self.config.repo_config.clone() {
                    if repo_config.auto_connect & self.config.repo_config.contains_key(&repo_name){
                        let stop_flag = Arc::new(atomic::AtomicBool::new(false));
                        let file_streaming_client_handle = self.start_stream(repo_name.to_string(), repo_config.watch_directory.to_string(), stop_flag.clone())?;
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
                stop_flag.store(true, atomic::Ordering::Relaxed);

                if let Err(_e) = handle.join() {
                    let message = format!("{} client thread failed to join", repo).to_string();
                    self.app_tx.send(Commands::Log(message.clone()))?;
                    self.app_tx.send(Commands::Notify(message.clone()))?;
                    return Err(anyhow::anyhow!(message.clone()));
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
            self.notify_app(&response)?;

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

    pub fn get_repo_tree(&mut self, repo_name:String) ->anyhow::Result<()>{
        let mut tree: Tree;
        let Some(existing_tree) = self.trees.get(&repo_name) else {
            self.app_tx.send(Commands::Log(
                format!("tree {} not found", repo_name)
            ))?;
            return Err(anyhow::anyhow!(format!("tree {} not found", repo_name)));
        };

        tree = existing_tree.clone();
        
        let body = json!({
            "repo_name": repo_name,
            "version": tree.version,
        });
        if let Some(stream) = self.command_stream.as_mut() {
            let request = Request {
                request_type: RequestTypes::GetRepoTree,
                body: serde_json::to_vec(&body)?,
            };

            send_request(request, stream)?;
            let response = read_response(stream)?;
            self.log_response(&response)?;
            self.notify_app(&response)?;

            if response.status_code == ResponseCodes::OK {
                if !response.body.is_empty() {
                    let tree_updates: HashMap<i32, String> =
                        serde_json::from_slice(&response.body)?;

                    let new_version = tree_updates.keys().cloned().max().unwrap_or(0);

                    self.app_tx.send(Commands::Log(
                        format!(
                            "Applying history from index {} to {}",
                            new_version - tree.version,
                            new_version
                        )
                    ))?;

                    self.app_tx.send(Commands::Notify(
                        format!(
                            "Applying history from index {} to {}",
                            new_version - tree.version,
                            new_version
                        )
                    ))?;
                    for (_, history_entry) in tree_updates {
                        tree.add_history(history_entry);
                    }

                    tree.apply_history(new_version);
                    tree.save_to_file(&tree.path);

                }

                self.app_tx.send(Commands::PostRepoTree(tree.clone(), repo_name))?;
            }
        }
        Ok(())
    }
}
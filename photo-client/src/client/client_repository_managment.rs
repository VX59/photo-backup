use super::Client;
use std::{collections::HashMap, path::PathBuf, sync::{Arc, atomic, mpsc}};
use shared::{FileHeader, Log, Notify, Request, RequestTypes, ResponseCodes, Tree, Job, BatchJob, read_response, send_request};
use crate::{app::{Commands, ConnectionStatus, RepoConfig}, filestreamclient::BatchLoaderCallback};
use serde_json::json;

impl Client {

    fn search_subdir(subdir_path:PathBuf, tree: &mut Tree, app_tx:&mpsc::Sender<Commands>, batch_loader_tx:&mpsc::Sender<BatchJob>) -> anyhow::Result<()>{
        let mut untracked_files = Vec::<String>::new();
        println!("subdir path is {}", subdir_path.to_string_lossy());
        for entry in std::fs::read_dir(&subdir_path)? {
            let entry = entry?;
            let name_os = entry.file_name();
            let path: PathBuf = entry.path();
            let file_type = entry.file_type()?;
            let name = match name_os.to_str() {
                Some(name) => name,
                None => continue
            };
            app_tx.send(Commands::Log(format!("currently in {}", &name)))?;
            if file_type.is_dir() {
                if !tree.content.contains_key(name) {
                    // start tracking this directory
                    tree.content.insert(name.to_string(), Vec::<String>::new());
                }
                Self::search_subdir(path, tree, app_tx, batch_loader_tx)?;

            } else if file_type.is_file() {
                // the tree will never have a file as a key so look in the contents of the tree

                let parent_name = match subdir_path.file_name().and_then(|os| os.to_str()) {
                    Some(name) => name,
                    None => continue, // skip file if parent dir is invalid UTF-8
                };

                let contents = tree.content.get(parent_name).map(|v: &Vec<String>| &v[..]).unwrap_or(&[]);

                if !contents.iter().any(|s| s == name) {
                    untracked_files.push(name.to_string());
                }
            }
        }
        let message = format!("found {} contains {:?}", subdir_path.to_string_lossy(), untracked_files);
        app_tx.send(Commands::Log(message))?;

        let mut jobs = Vec::<Job>::new();

        for file in untracked_files {
            let file_path = subdir_path.join(&file);

            let repo_name = &tree.name;
            let metadata = std::fs::metadata(&file_path)?;
            let file_datetime = metadata.created()?;
            let file_size= metadata.len();

            let file_ext = file_path.extension()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string();   
            
            let file_location = file_path
                .to_string_lossy()
                .into_owned();

            let file_header = FileHeader {
                repo_name: repo_name.to_string(),
                file_name: file,
                file_size: file_size as usize,
                file_location,
                file_ext,
                file_datetime,
            };

            let job = Job {
              file_header,
              data: std::fs::read(&file_path)? 
            };
            jobs.push(job);
        }
        if !jobs.is_empty() {
            let batch_job = BatchJob::new(jobs);
            batch_loader_tx.send(batch_job)?;
        }
        Ok(())
    }

    pub fn discover_untracked(&mut self, repo_name:String) -> anyhow::Result<()> {
        let watch_directory = match self.config.repo_config.get(&repo_name) {
            Some(c) => std::path::PathBuf::from(&c.watch_directory),
            None => return Ok(()) // add error
        };

        let mut temp_tree_copy = match self.trees.get(&repo_name) {
            Some(t) => t.clone(),
            None => return Err(anyhow::anyhow!("unable to locate tree for {}", &repo_name))
        };
        if let Some(batch_loader_tx) = &self.batch_loader_job_tx {
            if watch_directory.to_string_lossy().is_empty() {
                return Err(anyhow::anyhow!("watch directory is not saved in the config"))
            }
            Self::search_subdir(watch_directory, &mut temp_tree_copy, &self.app_tx, batch_loader_tx)?;
            
            if let Some(batch_loader_callback_rx) = &self.batch_loader_callback_rx {
                let mut stop_flag = false;
                while !stop_flag {
                    match batch_loader_callback_rx.try_recv() {
                        Ok(callback) => {
                            match callback {
                                BatchLoaderCallback::Done => {
                                    stop_flag = true;
                                },
                                BatchLoaderCallback::Failed => {},
                            }
                        },
                        Err(std::sync::mpsc::TryRecvError::Empty) => {},
                        Err(e) => return Err(anyhow::anyhow!(e))
                    };
                };

                self.get_repo_tree(repo_name)?;
            } else {
                return Err(anyhow::anyhow!("batch loader callback reciever not available"))
            }

        } else {
            return Err(anyhow::anyhow!("batch loader tx is not available"));
        }
        Ok(())
    }

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
                        let file_streaming_client_handle = self.start_event_listener(repo_name.to_string(), repo_config.watch_directory.to_string(), stop_flag.clone())?;
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
                self.app_tx.send(Commands::UpdateRepoStatus((repo.clone(),ConnectionStatus::Disconnected)))?;
            },
            None => {
                self.app_tx.send(Commands::Log(format!("Not connected to {}", repo).to_string()))?;
            }
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

                if let Some(tree) = self.trees.get(&repo_name.clone()) {
                    std::fs::remove_file(&tree.path)?;
                }

                self.config.save_to_file("./photo-client-config.json");
                self.app_tx.send(Commands::RemoveRepository(repo_name.to_string()))?;
            }            
        }
        Ok(())
    }

    pub fn get_repo_tree(&mut self, repo_name:String) ->anyhow::Result<()>{
        let mut tree: Tree;
        if let Some(existing_tree) = self.trees.get(&repo_name) {
            tree = existing_tree.clone();
        } else {
            self.app_tx.send(Commands::Log(format!("{} tree not found, initializing tree", repo_name.clone())))?;
            tree = Tree {
                    version: 0,
                    content: HashMap::new(),
                    history: HashMap::new(),
                    path: ("trees".to_string() + "/" + &repo_name + ".tree").to_string(),
                    name: repo_name.clone(),
                };
            self.trees.insert(repo_name.clone(), tree.clone());
        };
        
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

                    let start_index = tree.version;
                    for (_, history_entry) in tree_updates {
                        tree.add_history(history_entry);
                    }
                    
                    println!("applying history from {} to {}", start_index, tree.version);
                    tree.apply_history(start_index);
                    self.trees.insert(repo_name.clone(), tree.clone());
                    tree.save_to_file(&tree.path);

                }
                
                self.app_tx.send(Commands::PostRepoTree(tree.clone(), repo_name))?;
            }
        }
        Ok(())
    }
}
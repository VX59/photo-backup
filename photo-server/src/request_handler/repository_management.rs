use std::{collections::HashMap, path::Path};
use serde_json;
use shared::{send_response, Request, Response, ResponseCodes, Tree};

use super::PhotoServerRequestHandler;

impl PhotoServerRequestHandler {
    pub fn remove_repository(&mut self, request:Request) -> anyhow::Result<()> {
        let repo_name = String::from_utf8_lossy(&request.body)
            .trim() 
            .replace(|c: char| c.is_control(), "_")
            .to_string();

        self.config.remove_repo(repo_name.clone());
        if let Some(tree) = self.trees.get(&repo_name) {
            std::fs::remove_file(tree.clone().path)?;
        }

        let repo_path = std::path::Path::new(&self.config.storage_directory).join(repo_name.clone());
        std::fs::remove_dir_all(repo_path)?;

        let response = Response {
            status_code: ResponseCodes::OK,
            status_message: "OK".to_string(),
            body: format!("{} repo successfully deleted",repo_name).as_bytes().to_vec(),
        };
        send_response(response, &mut self.stream)?;
        Ok(())
    }

    pub fn create_repo(&mut self, request:Request) -> anyhow::Result<()> {
        let repo_name = String::from_utf8_lossy(&request.body)
            .trim()         // removes leading/trailing whitespace
            .replace(|c: char| c.is_control(), "_") // replace control chars with _
            .to_string(); 
        let repo_path = Path::new(&self.config.storage_directory).join(&repo_name);
        
        let response:Response;

        if repo_path.exists() {
            response = Response {
                status_code: ResponseCodes::Duplicate,
                status_message: "Err".to_string(),
                body: "A repo with the same name already exists".as_bytes().to_vec()
            };
        } else {

            let mut response_message = format!("Successfully created new repository | {}", repo_name);
            let mut status_code = ResponseCodes::OK;
            let mut status_message = "OK";
            if let Err(e) = std::fs::create_dir(repo_path) {
                response_message = e.to_string();
                status_code = ResponseCodes::InternalError;
                status_message = "Failed to create the repo directory";
            }

            // save it to the config
            self.config.add_repo(repo_name.clone());
            
            // load a tree
            let tree:Tree = Tree {
                version: 0,
                content: HashMap::new(),
                history: HashMap::new(),
                path: ("trees".to_string() + "/" + &repo_name + ".tree").to_string(),
                name: repo_name.clone(),
            };
            tree.save_to_file(&tree.path);
            self.trees.insert(repo_name, tree);
            
            response = Response {
                status_code: status_code,
                status_message: status_message.to_string(),
                body: response_message.as_bytes().to_vec(),
            }
        }
        
        send_response(response,&mut self.stream)?;
        Ok(())
    }

    pub fn get_repo_tree(&mut self, request:Request) -> anyhow::Result<()> {
        let body = serde_json::from_slice::<HashMap<String, serde_json::Value>>(&request.body)?;
        let repo_name = body.get("repo_name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let version = body.get("version")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as i32;

        let response: Response;
        //update the tree from the disk because the file streaming server has a different tree that is at least as up to date as this one
        self.trees.insert(repo_name.clone(), Tree::load_from_file(&("trees".to_string() + "/" + &repo_name + ".tree").to_string()));
        if let Some(tree) = self.trees.get(&repo_name) {
            if tree.version > version {
                let updates = tree.history.iter()
                    .filter(|(v, _)| {
                        if version > 0 {
                            *v > &version
                        } else {
                            *v >= &version
                        }
                        })
                    .map(|(&v, entry)| (v, entry.clone()))
                    .collect::<HashMap<i32, String>>();
                
                let response_body = serde_json::to_vec(&updates)?;
                response = Response {
                    status_code: ResponseCodes::OK,
                    status_message: "Missing updates".to_string(),
                    body: response_body,
                };
            }
            else {
                response = Response {
                    status_code: ResponseCodes::OK,
                    status_message: "No updates".to_string(),
                    body: Vec::new(),
                };
            }
        } else {
            response = Response {
                status_code: ResponseCodes::NotFound,
                status_message: "Tree not found".to_string(),
                body: Vec::new(),
            };
        }

        send_response(response, &mut self.stream)?;
        Ok(())
    }

    pub fn disconnect_stream(&mut self, request:Request) -> anyhow::Result<()> {
        let repo_name = String::from_utf8_lossy(&request.body)
            .trim()         // removes leading/trailing whitespace
            .replace(|c: char| c.is_control(), "_") // replace control chars with _
            .to_string();            

        let response:Response = match self.repo_threads.remove(&repo_name) {
            Some((handle, stop_flag)) => {
                stop_flag.store(true, std::sync::atomic::Ordering::Relaxed);
                
                if let Err(_e) = handle.join() {
                    Response { 
                        status_code: ResponseCodes::InternalError, 
                        status_message: "Err".to_string(),
                        body: format!("{} server thread failed to terminate", repo_name).as_bytes().to_vec(),
                    }
                } else {
                    Response { 
                        status_code: ResponseCodes::OK, 
                        status_message: "OK".to_string(),
                        body: format!("Successfully disconnected from {}", repo_name).as_bytes().to_vec(),
                    }
                }
            },
            None => {
                Response { 
                    status_code: ResponseCodes::NotFound, 
                    status_message: "Err".to_string(),
                    body: format!("{} server thread failed to terminate : not found", repo_name).as_bytes().to_vec(),
                }
            }
        };
        send_response(response, &mut self.stream)?;
        Ok(())
    }

    pub fn get_preview(&mut self, request:Request) -> anyhow::Result<()> {
        Ok(())
    }

}
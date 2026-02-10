use std::{collections::HashMap, path::Path};
use serde_json;
use shared::{send_response, Request, Response, ResponseCodes, Tree};
use rand::Rng;
use crate::filestreamserver::{initiate_batch_processor};
use std::{net::{TcpListener}, sync::{Arc,atomic}, path::PathBuf};

use super::PhotoServerRequestHandler;

impl PhotoServerRequestHandler {

    pub fn remove_repository(&mut self, request:Request) -> anyhow::Result<()> {
        let repo_name = String::from_utf8_lossy(&request.body)
            .trim() 
            .replace(|c: char| c.is_control(), "_")
            .to_string();

        self.config.remove_repo(repo_name.clone());
        if let Some(tree) = self.trees.get(&repo_name) {
            std::fs::remove_file(&tree.path)?;
        }

        let repo_path = std::path::Path::new(&self.config.storage_directory).join(&repo_name);
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
            self.config.save_to_file(&self.config.config_path);
            
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

    pub fn start_batch_processor(&mut self) -> anyhow::Result<()> {    
            let port = rand::rng().random_range(0..u16::MAX);
            let file_stream_address = format!("0.0.0.0:{}", port);

            let listener = TcpListener::bind(&file_stream_address)?;
            
            let response = Response {
                status_code:ResponseCodes::OK,
                status_message: format!("Initiated file stream @ {}", &file_stream_address).to_string(),
                body: file_stream_address.as_bytes().to_vec(),
            };

            send_response(response, &mut self.stream)?;

            let stop_flag = Arc::new(atomic::AtomicBool::new(false));
            match initiate_batch_processor(PathBuf::from(&self.config.storage_directory), listener,stop_flag.clone()) {
                
                Ok(handle) => { 
                    self.batch_processor_context = Some((handle, stop_flag))
                },
                Err(e) => {
                    let response = Response {
                        status_code:ResponseCodes::InternalError,
                        status_message:"Err".to_string(),
                        body: "Failed to batch processor".as_bytes().to_vec(),
                    };
                    send_response(response, &mut self.stream)?;
                    return Err(anyhow::anyhow!(format!("{}",e)));
                },
            };
            Ok(())
        }

    pub fn end_batch_processor(&mut self) -> anyhow::Result<()> {           
        if let Some((handle, stop_flag)) = self.batch_processor_context.take() {
            stop_flag.store(true, std::sync::atomic::Ordering::Relaxed);
            let response:Response;
            if let Err(_e) = &handle.join() {
                response = Response { 
                    status_code: ResponseCodes::InternalError, 
                    status_message: "Err".to_string(),
                    body: "{} batch processor failed to terminate".as_bytes().to_vec(),
                };
            } else {
                response = Response { 
                    status_code: ResponseCodes::OK, 
                    status_message: "OK".to_string(),
                    body: "Successfully terminated batch processor".as_bytes().to_vec(),
                };
            }
            
            send_response(response, &mut self.stream)?;
        }
        Ok(())
    }
}
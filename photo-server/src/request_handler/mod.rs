use std::{collections::HashMap, net::{TcpStream}, sync::{Arc,atomic}};
use serde_json;
use shared::{read_request, send_response, Request, RequestTypes, Response, ResponseCodes, Tree};

use request_handler_utils::ServerConfig;

pub mod request_handler_utils;
mod server_repository_management;
pub struct PhotoServerRequestHandler {
    pub stream:TcpStream,
    pub config:ServerConfig,
    pub batch_processor_context: Option<(std::thread::JoinHandle<()>, Arc<atomic::AtomicBool>)>,
    pub trees:HashMap<String, Tree>
}

impl PhotoServerRequestHandler {
    pub fn new(config_path:String, stream:TcpStream) -> Self {
        PhotoServerRequestHandler {
            stream,
            config: ServerConfig::load_from_file(&config_path),
            batch_processor_context: None,
            trees: HashMap::new(),
        }
    }

    pub fn run(&mut self) -> anyhow::Result<()> {
        println!("Launching a request handler");
        for entry in std::fs::read_dir("trees")? {
            let entry = entry?;
            let path = entry.path();
            let tree = Tree::load_from_file(path.to_str().unwrap());
            let repo_name = path.file_stem().unwrap().to_string_lossy().to_string();
            self.trees.insert(repo_name, tree);
        }
        loop {
            let request = read_request(&mut self.stream)?;
            match request.request_type {
                RequestTypes::GetRepos => self.get_repos()?,
                RequestTypes::CreateRepo => self.create_repo(request)?,
                RequestTypes::StartBatchProcessor => self.start_batch_processor()?,
                RequestTypes::EndBatchProcessor => self.end_batch_processor()?,
                RequestTypes::RemoveRepository => self.remove_repository(request)?,
                RequestTypes::GetRepoTree => self.get_repo_tree(request)?,
                RequestTypes::SetStoragePath => self.set_storage_path(request)?,
            }
        }
    }

    fn set_storage_path(&mut self, request:Request) -> anyhow::Result<()> {
            let storage_directory = String::from_utf8_lossy(&request.body)
                .trim() 
                .replace(|c: char| c.is_control(), "_")
                .to_string();
            
            let storage_directory_path = std::path::Path::new(&storage_directory);

            let response:Response;
            if storage_directory_path.exists() == false {
                
                response = Response {
                    status_code: ResponseCodes::NotFound,
                    status_message: "Invalid path".to_string(),
                    body: "Invalid Global Storage Path".as_bytes().to_vec(),
                };

            } else {
                self.config.storage_directory = storage_directory;
                self.config.save_to_file(&self.config.config_path);
                
                response = Response {
                    status_code:ResponseCodes::OK,
                    status_message:"".to_string(),
                    body: "Successfully set the storage directory path".as_bytes().to_vec()
                }
            }
            send_response(response, &mut self.stream)?;
            Ok(())
    }

    fn get_repos(&mut self) -> anyhow::Result<()> {
        let response:Response;

        if self.config.repo_list.is_empty() {
            response = Response {
                status_code: ResponseCodes::Empty,
                status_message: "Empty config".to_string(),
                body: "There are no available repositories. The server will wait until you create one".as_bytes().to_vec(),
            };

        } else {
            let available_repositories = self.config.repo_list.clone();

            response  = Response {
                status_code: ResponseCodes::OK,
                status_message: "OK".to_string(),
                body: serde_json::to_vec(&available_repositories)?,
            };
        }
        
        send_response(response, &mut self.stream)?;
        Ok(())
    }
}
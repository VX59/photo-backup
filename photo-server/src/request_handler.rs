use std::{
collections::HashMap, net::{TcpListener, TcpStream}, path::Path,
};
use serde::{Deserialize, Serialize};
use rand::{Rng};
use serde_json;
use crate::filestreamserver::{initiate_file_streaming_server};
use shared::{read_request, send_response, Request, RequestTypes, Response, ResponseCodes, Tree};

#[derive(Serialize,Deserialize, Default, Debug, Clone)]
pub struct Config {
    pub storage_directory:String,
    pub repo_list: Vec<String>,
    pub config_path: String,
}

impl Config {
    pub fn load_from_file(path: &str) -> Self {
        let config_content = std::fs::read_to_string(path)
        .unwrap_or_else(|_| {
            println!("Config file not found, using default configuration.");
            String::new()
        });
        serde_json::from_str(&config_content).unwrap_or_else(|_| {
            println!("Failed to parse config file, using default configuration.");
            Config::default()
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

pub struct PhotoServerRequestHandler {
    pub stream:TcpStream,
    pub config:Config,
    pub repo_threads: HashMap<String, (std::thread::JoinHandle<()>, std::sync::Arc<std::sync::atomic::AtomicBool>)>,
}

impl PhotoServerRequestHandler {
    pub fn new(config_path:String, stream:TcpStream) -> Self {
        PhotoServerRequestHandler {
            stream,
            config: Config::load_from_file(&config_path),
            repo_threads: HashMap::new(),
        }
    }

    pub fn run(&mut self) -> anyhow::Result<()> {
        println!("Launching a request handler");
        loop {
            let request = read_request(&mut self.stream)?;
            match request.request_type {
                RequestTypes::GetRepos => self.get_repos()?,
                RequestTypes::CreateRepo => self.create_repo(request)?,
                RequestTypes::StartStream => self.start_stream(request)?,
                RequestTypes::DisconnectStream => self.disconnect_stream(request)?,
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

    fn remove_repository(&mut self, request:Request) -> anyhow::Result<()> {
        let repo_name = String::from_utf8_lossy(&request.body)
            .trim() 
            .replace(|c: char| c.is_control(), "_")
            .to_string();

        self.config.remove_repo(repo_name.clone());
        let tree = Tree::load_from_file(&("trees".to_string() + "/" + &repo_name + ".tree").to_string());
        std::fs::remove_file(tree.path)?;
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

    fn create_repo(&mut self, request:Request) -> anyhow::Result<()> {
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
            
            response = Response {
                status_code: status_code,
                status_message: status_message.to_string(),
                body: response_message.as_bytes().to_vec(),
            }
        }
        
        send_response(response,&mut self.stream)?;
        Ok(())
    }

    fn start_stream(&mut self, request:Request) -> anyhow::Result<()> {
        let repo_name = String::from_utf8_lossy(&request.body)
            .trim()         // removes leading/trailing whitespace
            .replace(|c: char| c.is_control(), "_") // replace control chars with _
            .to_string();                 

        let port = rand::rng().random_range(0..u16::MAX);
        let file_stream_address = format!("0.0.0.0:{}", port);

        let listener = TcpListener::bind(&file_stream_address)?;
        
        let response = Response {
            status_code:ResponseCodes::OK,
            status_message: format!("Initiated file stream @ {}", &file_stream_address).to_string(),
            body: file_stream_address.as_bytes().to_vec(),
        };

        send_response(response, &mut self.stream)?;

        let stop_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        match initiate_file_streaming_server(repo_name.clone(), self.config.storage_directory.clone(), listener,stop_flag.clone()) {
            
            Ok(handle) => { 
                self.repo_threads.insert(repo_name.clone(),(handle, stop_flag));
            },
            Err(e) => {
                let response = Response {
                    status_code:ResponseCodes::InternalError,
                    status_message:"Err".to_string(),
                    body: format!("Failed to initiate repository {}", repo_name).as_bytes().to_vec(),
                };
                send_response(response, &mut self.stream)?;
                return Err(anyhow::anyhow!(format!("{}",e)));
            },
        };
        Ok(())
    }

    fn disconnect_stream(&mut self, request:Request) -> anyhow::Result<()> {
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

    fn get_repo_tree(&mut self, request:Request) -> anyhow::Result<()> {
        let body = serde_json::from_slice::<HashMap<String, serde_json::Value>>(&request.body)?;
        let repo_name = body.get("repo_name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let version = body.get("version")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as i32;

        let tree = Tree::load_from_file(&("trees".to_string() + "/" + &repo_name + ".tree").to_string());

        let response: Response;
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

        send_response(response, &mut self.stream)?;
        Ok(())
    }
}
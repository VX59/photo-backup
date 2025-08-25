use std::{
collections::HashMap, net::{TcpListener, TcpStream}
};
use rand::{Rng};
use std::path::Path;
use serde::Deserialize;
use serde::Serialize;
use crate::filestreamserver::{initiate_file_streaming_server};
use shared::{read_request, send_response, RequestTypes, Response, ResponseCodes};

#[derive(Serialize,Deserialize, Default, Debug, Clone)]
pub struct Config {
    pub repo_list: Vec<String>,
    pub path: String,
}

pub struct PhotoServer {
    pub name: String,
    pub address: String,
    pub storage_directory: String,
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

    pub fn add_repo(&mut self, repo:String) {
        if !self.repo_list.contains(&repo) {
            self.repo_list.push(repo);
            self.save_to_file(&self.path);
        } else {
            eprintln!("Repo already exists in config.");
        }
    }
}

impl PhotoServer {
    pub fn new(name: String, address: String, storage_directory: String) -> Self {
        PhotoServer {
            name,
            address,
            storage_directory,
        }
    }

    pub fn start(&mut self) -> std::io::Result<()> {

        let listener = TcpListener::bind("0.0.0.0:8080")?;
        println!("Photo server listening on {}", self.address);
        
        for stream in listener.incoming() {
            let mut stream = match stream {
                Ok(s) => s,
                Err(e) => return Err(e)
            };

            println!("New connection: {}", stream.peer_addr().expect("Failed to get peer address"));

            let response = Response {
                status_code: ResponseCodes::OK,
                status_message: "OK".to_string(),
                body: format!("connected to photo server @ {}", self.name).as_bytes().to_vec(),
            };

            send_response(response, &mut stream)?;

            // Handle storage directory request

            let storage_directory_request = read_request(&mut stream)?;
            let storage_directory_request_message = String::from_utf8_lossy(&storage_directory_request.body);
            let storage_directory_path = Path::new(storage_directory_request_message.as_ref());
            
            if storage_directory_path.exists() == false {
                
                let response = Response {
                    status_code: ResponseCodes::NotFound,
                    status_message: "Invalid path".to_string(),
                    body: "Closing connection due to invalid repository path.".as_bytes().to_vec(),
                };

                send_response(response, &mut stream)?;
                drop(stream);
                continue;
            }
            
            let response = Response {
                status_code: ResponseCodes::OK,
                status_message: "OK".to_string(),
                body: format!("Global storage path confirmed {}", storage_directory_request_message).as_bytes().to_vec(),
            };
            send_response(response, &mut stream)?;
            
            self.storage_directory = storage_directory_request_message.to_string();
            
            let storage_directory_clone = self.storage_directory.clone();
            
            // spawn a request handler in a seperate thread so we can accept another connection
            let _ = std::thread::spawn(move || {
                if let Err(e) = request_handler(storage_directory_clone, stream) {
                    println!("{}", e);
                }
            });
        }
        
        Ok(())
    }

}

fn request_handler(storage_directory: String, mut stream:TcpStream) -> std::io::Result<()>{
    println!("Launching a request handler");
    let mut config = Config::load_from_file("./photo-server-config.json");
    let mut repo_threads: HashMap<String, (std::thread::JoinHandle<()>, std::sync::Arc<std::sync::atomic::AtomicBool>)> = HashMap::new();
    loop {
        let request = read_request(&mut stream)?;
        match request.request_type {
            RequestTypes::GetRepos => {
                let response:Response;

                if config.repo_list.is_empty() {
                    response = Response {
                        status_code: ResponseCodes::Empty,
                        status_message: "Empty config".to_string(),
                        body: "There are no available repositories. The server will wait until you create one".as_bytes().to_vec(),
                    };

                } else {
                    let available_repositories = &config.repo_list;

                    response  = Response {
                        status_code: ResponseCodes::OK,
                        status_message: "OK".to_string(),
                        body: serde_json::to_vec(&available_repositories)?,
                    };
                }
                
                send_response(response, &mut stream)?;
            }
            RequestTypes::CreateRepo => {
                let repo_name = String::from_utf8_lossy(&request.body)
                    .trim()         // removes leading/trailing whitespace
                    .replace(|c: char| c.is_control(), "_") // replace control chars with _
                    .to_string(); 
                let repo_path = Path::new(&storage_directory).join(&repo_name);
                
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
                    config.add_repo(repo_name);

                    response = Response {
                        status_code: status_code,
                        status_message: status_message.to_string(),
                        body: response_message.as_bytes().to_vec(),
                    }
                }
                
                send_response(response,&mut stream)?;
            },
            RequestTypes::StartStream => {
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

                send_response(response, &mut stream)?;

                let stop_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                match initiate_file_streaming_server(repo_name.clone(), storage_directory.clone(), listener,stop_flag.clone()) {
                    
                    Ok(handle) => { repo_threads.insert(repo_name,(handle, stop_flag)) },
                    Err(e) => {
                        let response = Response {
                            status_code:ResponseCodes::OK,
                            status_message:"OK".to_string(),
                            body: format!("Failed to initiate repository {}", repo_name).as_bytes().to_vec(),
                        };

                        send_response(response, &mut stream)?;
                        return Err(e);
                    },
                };
            },

            RequestTypes::DisconnectStream => {
                let repo_name = String::from_utf8_lossy(&request.body)
                    .trim()         // removes leading/trailing whitespace
                    .replace(|c: char| c.is_control(), "_") // replace control chars with _
                    .to_string();            

                let response:Response = match repo_threads.remove(&repo_name) {
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

                send_response(response, &mut stream)?;
            },

            _ => {},
        }

    }
}
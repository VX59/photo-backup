use std::{
    io::{prelude::*, BufReader},
    net::{TcpListener, TcpStream}, thread, time::Duration,
};

use rand::{rng, Rng};
use anyhow::Result;
use std::path::Path;
use::bincode::{config};
use::shared::FileHeader;
use serde::Deserialize;
use serde::Serialize;

use shared::{read_request, send_response, RequestTypes, Response};

#[derive(Serialize,Deserialize, Default, Debug, Clone)]
pub struct Config {
pub repo_list: Vec<String>,
}

pub struct PhotoServer {
    pub name: String,
    pub address: String,
    pub storage_directory: String,
    pub config: Config,
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

    pub fn add_repo(&mut self, repo:String, path: &str) {
        if !self.repo_list.contains(&repo) {
            self.repo_list.push(repo);
            self.save_to_file(path);
        } else {
            eprintln!("Repo already exists in config.");
        }
    }
}

impl PhotoServer {
    pub fn new(name: String, address: String, storage_directory: String, config:Config) -> Self {
        PhotoServer {
            name,
            address,
            storage_directory,
            config,
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

            let mut response = Response {
                status_code: 200,
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
                    status_code: 400,
                    status_message: "Invalid path".to_string(),
                    body: "Closing connection due to invalid repository path.".as_bytes().to_vec(),
                };

                send_response(response, &mut stream)?;
                drop(stream);
                continue;
            } 

            // load available repositories

            if self.config.repo_list.is_empty() {
                response = Response {
                    status_code: 300,
                    status_message: "Empty config".to_string(),
                    body: "There are no available repositories. The server will wait until you create one".as_bytes().to_vec(),
                };

            } else {
                let available_repositories = &self.config.repo_list;

                response  = Response {
                    status_code: 200,
                    status_message: "OK".to_string(),
                    body: serde_json::to_vec(&available_repositories)?,
                };
            }
            
            send_response(response, &mut stream)?;
            
            self.storage_directory = storage_directory_request_message.to_string();
            
            let storage_directory_clone = self.storage_directory.clone();
            let mut config_clone = self.config.clone();
            // spawn a request handler in a seperate thread so we can accept another connection
            let _ = std::thread::spawn(move || {
                if let Err(e) = request_handler(storage_directory_clone, stream, &mut config_clone) {
                    println!("{}", e);
                }
            });
        }
        
        Ok(())
    }

}

fn request_handler(storage_directory: String, mut stream:TcpStream, config: &mut Config) -> std::io::Result<()>{
    println!("Launching a request handler");
    loop {
        let request = read_request(&mut stream)?;
        match request.request_type {
            RequestTypes::CreateRepo => {
                let repo_name = String::from_utf8_lossy(&request.body)
                    .trim()         // removes leading/trailing whitespace
                    .replace(|c: char| c.is_control(), "_") // replace control chars with _
                    .to_string(); 
                let repo_path = Path::new(&storage_directory).join(&repo_name);
                
                let response:Response;

                if repo_path.exists() {
                    response = Response {
                      status_code: 500,
                      status_message: "Err".to_string(),
                      body: "A repo with the same name already exists".as_bytes().to_vec()
                    };
                } else {

                    let mut response_message = format!("Successfully created new repository | {}", repo_name);
                    let mut status_code = 200;
                    let mut status_message = "OK";
                    if let Err(e) = std::fs::create_dir(repo_path) {
                        response_message = e.to_string();
                        status_code = 600;
                        status_message = "Err";
                    }

                    // save it to the config
                    config.add_repo(repo_name, "photo-server-json.config");

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
                let repo_path = Path::new(&storage_directory).join(&repo_name);

                let port = rand::rng().random_range(0..u16::MAX);
                let file_stream_address = format!("0.0.0.0:{}", port);

                let listener = TcpListener::bind(&file_stream_address)?;
                
                let response = Response {
                    status_code:200,
                    status_message: format!("Initiated file stream @ {}", &file_stream_address).to_string(),
                    body: file_stream_address.as_bytes().to_vec(),
                };

                send_response(response, &mut stream)?;

                match listener.accept() {
                    Ok((mut file_stream, socket_addr)) => {
                        let response = Response {
                            status_code:200,
                            status_message:"OK".to_string(),
                            body: format!("{} connected to repository {}", socket_addr, repo_name).as_bytes().to_vec(),
                        };

                        send_response(response, &mut file_stream)?;

                        // move the file stream to its own thread
                        let file_stream_handle = std::thread::spawn(move || {
                            println!("file stream thread initiated");
                            photo_upload_handler(file_stream, repo_path.as_path());
                        });
                    },
                    Err(e) => return Err(e),
                }
            },

            RequestTypes::SetStoragePath => {},
        }

    }
}

fn photo_upload_handler(mut stream: TcpStream, file_dest: &Path) {
    loop {
        let mut reader = BufReader::new(&stream);
        match upload_image(&mut reader, file_dest) {
            Ok(file_name) => {
                let response = format!("received {:?}: HTTP/1.1 200 OK\r\n", file_name);
                if let Err(_e) = stream.write_all(response.as_bytes()) {
                    println!("Failed to write response to stream");
                    break;
                }
            }
            Err(e) => {
                println!("Connection closed or error: {:?}", e);
                break;
            }
        }
    }
}

fn upload_image(reader:&mut BufReader<&TcpStream>, file_dest: &Path) -> Result<String, anyhow::Error> {
    let mut header_length_buffer = [0u8; 4];
    // Read the request line

    reader.read_exact(&mut header_length_buffer)?;

    let header_length = u32::from_be_bytes(header_length_buffer);
    println!("Header length: {}", header_length);
    let mut header_bytes = vec![0u8; header_length as usize];

    reader.read_exact(&mut header_bytes)?;
    
    let (file_header, _): (FileHeader, _) = bincode::decode_from_slice(&header_bytes, config::standard())?;

    let mut image_bytes = vec![0u8; file_header.file_size as usize];
    println!("Receiving file: {} ({} bytes)", file_header.file_name, file_header.file_size);

    reader.read_exact(&mut image_bytes)?;

    let image = image::load_from_memory(&image_bytes)?;

    let image_path = file_dest.join(file_header.file_name);
    if image_path.exists() {
        // save it to the proper subdirectory.. triggered by recursive backup
        if std::path::Path::new(&image_path).exists() == false {
            std::fs::create_dir_all(&image_path)?;
        }
    }

    image.save(std::path::Path::new(&image_path))?;
    Ok(image_path.to_string_lossy().to_string())

}
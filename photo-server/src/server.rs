use std::{
    io::{BufReader, prelude::*},
    net::{TcpListener, TcpStream},
};

use std::path::Path;
use::bincode::{config};
use::shared::FileHeader;
use serde::Deserialize;
use serde::Serialize;

use shared::{read_response, send_response, Response};

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

            let storage_directory_request = read_response(&mut stream)?;
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
                    body: match serde_json::to_vec(&available_repositories) {
                        Ok(json_body) => json_body,
                        Err(e) => return Err(std::io::Error::new(std::io::ErrorKind::Other,e)),
                    },
                };
            }
            
            send_response(response, &mut stream)?;
           
            let repository_path_request = read_response(&mut stream)?;
            let repository_path_request_message = String::from_utf8_lossy(&repository_path_request.body);
            println!("{}",repository_path_request_message);
            
            self.storage_directory = storage_directory_request_message.to_string();

            // await repo selection

        }
        
        Ok(())
    }

    fn photo_upload_handler(&mut self, mut stream: TcpStream) {
        loop {
            let mut reader = BufReader::new(&stream);
            match self.upload_image(&mut reader) {
                Ok(file_name) => {
                    let response = format!("{} received {}: HTTP/1.1 200 OK\r\n", self.name, file_name);
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

    fn upload_image(&mut self, reader:&mut BufReader<&TcpStream>) ->std::io::Result<String> {
        let mut header_length_buffer = [0u8; 4];
        // Read the request line

        if let Err(e) = reader.read_exact(&mut header_length_buffer) {
            println!("Failed to read from stream: {}", e);
            return Err(e);
        }

        let header_length = u32::from_be_bytes(header_length_buffer);
        println!("Header length: {}", header_length);
        let mut header_bytes = vec![0u8; header_length as usize];

        if let Err(e) = reader.read_exact(&mut header_bytes) {
            println!("Failed to read image header from stream: {}", e);
            return Err(e);
        }

        

        let (file_header, _): (FileHeader, _) = match bincode::decode_from_slice(&header_bytes, config::standard()) {
            Ok(res) => res,
            Err(e) => return Err(std::io::Error::new(std::io::ErrorKind::Other, e))
        };

        let mut image_bytes = vec![0u8; file_header.file_size as usize];
        println!("Receiving file: {} ({} bytes)", file_header.file_name, file_header.file_size);

        if let Err(e) = reader.read_exact(&mut image_bytes) {
            println!("Failed to read image header from stream");
            return Err(e);
        }   

        let image = match image::load_from_memory(&image_bytes) {
            Ok(i) => i,
            Err(e) => {
                println!("Failed to decode image");
                return Err(std::io::Error::new(std::io::ErrorKind::Other, e));
            }
        };

        let mut image_path = format!("{}/{}",self.storage_directory.clone(), file_header.file_name);
        if file_header.file_dest != "..." {
            // save it to the proper subdirectory.. triggered by recursive backup
            image_path = format!("{}/{}/{}", self.storage_directory, file_header.file_dest, file_header.file_name);
            if std::path::Path::new(&image_path).exists() == false {
                std::fs::create_dir_all(&image_path)?;
            }
        }

        match image.save(std::path::Path::new(&image_path)) {
            Ok(_) => return Ok(file_header.file_name),
            Err(e) => return Err(std::io::Error::new(std::io::ErrorKind::Other, e))
        }

    }
}
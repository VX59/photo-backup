use std::{
net::{TcpListener}
};
use std::path::Path;

use shared::{read_request, send_response, Response, ResponseCodes};
use crate::request_handler::PhotoServerRequestHandler;

pub struct PhotoServer {
    pub name: String,
    pub address: String,
    pub storage_directory: String,
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
        if std::path::Path::new("photo-server/trees").exists() == false {
            std::fs::create_dir_all("trees")?;
        }

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
                let mut request_handler = PhotoServerRequestHandler::new(
                    "./photo-server-config.json".to_string(),
                    stream,
                    storage_directory_clone);
                
                if let Err(e) = request_handler.run() {
                    println!("{}", e);
                }
            });
        }
        
        Ok(())
    }

}
use std::net::TcpListener;

use shared::{send_response, Response, ResponseCodes};
use crate::request_handler::PhotoServerRequestHandler;

pub struct PhotoServer {
    pub name: String,
    pub address: String,
}

impl PhotoServer {
    pub fn new(name: String, address: String,) -> Self {
        PhotoServer {
            name,
            address,
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
            
            // spawn a request handler in a seperate thread so we can accept another connection
            let _ = std::thread::spawn(move || {
                let mut request_handler = PhotoServerRequestHandler::new(
                    "./photo-server-config.json".to_string(),
                    stream);
                
                if let Err(e) = request_handler.run() {
                    println!("{}", e);
                }
            });
        }
        
        Ok(())
    }

}
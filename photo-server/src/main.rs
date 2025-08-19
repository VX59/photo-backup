use std::{
    io::{BufReader, prelude::*},
    net::{TcpListener, TcpStream},
};

use std::path::Path;
use::bincode::{config};
use::shared::FileHeader;
use hostname::get;


pub struct Response {
    pub status_code: u16,
    pub status_message: String,
    pub body: Vec<u8>,
}

pub struct PhotoServer {
    pub name: String,
    pub address: String,
    pub repo_path: String,
}

impl PhotoServer {
    pub fn new(name: String, address: String, repo_path: String) -> Self {
        PhotoServer {
            name,
            address,
            repo_path,
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
                status_code: 200,
                status_message: "OK".to_string(),
                body: format!("connected to photo server @ {}", self.name).as_bytes().to_vec(),
            };

            if let Err(e) = stream.write_all(response.body.as_slice()) {
                return Err(e)
            };

            // read the repository path from client
            let mut repo_path_buffer = vec![0u8; 256];

            let n = match stream.read(&mut repo_path_buffer) {
                Ok(n) => n,
                Err(e) => return Err(e)
            };

            let repo_path_str = String::from_utf8_lossy(&repo_path_buffer[..n]);
            let repo_path = Path::new(repo_path_str.as_ref());

            let mut response_body = format!("OK Repository path : '{}'", repo_path_str).as_bytes().to_vec();
            if repo_path.exists() == false {
                response_body = format!("Repository path does not exist on server: {}", repo_path_str).as_bytes().to_vec();
            }

            let response  = Response {
                status_code: 200,
                status_message: "OK".to_string(),
                body: response_body,
            };

            if let Err(e) = stream.write_all(response.body.as_slice()) {
                return Err(e)
            }
            stream.flush()?;

            if repo_path.exists() == false {
                println!("Closing connection due to invalid repository path.");
                drop(stream);
                continue;
            }

            self.repo_path = repo_path_str.to_string();

            self.photo_upload_handler(stream);
        }
        
        Ok(())
    }

    fn photo_upload_handler(&mut self, mut stream: TcpStream) {
        loop {
            let mut reader = BufReader::new(&stream);
            let name = get().unwrap_or_default().to_string_lossy().to_string();
            match self.upload_image(&mut reader) {
                Ok(file_name) => {
                    let response = format!("{} received {}: HTTP/1.1 200 OK\r\n", name, file_name);
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

        let mut image_path = format!("{}/{}",self.repo_path.clone(), file_header.file_name);
        if file_header.file_dest != "..." {
            // save it to the proper subdirectory.. triggered by recursive backup
            image_path = format!("{}/{}/{}", self.repo_path, file_header.file_dest, file_header.file_name);
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

fn main() {

    // sets a default repo path in case we dont want to make a custom one for now

    let name = get().unwrap_or_default().to_string_lossy().to_string(); // gets the servers hostname dynamically
    let mut photo_server = PhotoServer::new(
        name.clone(),
        format!("{}:{}", name, "8080"),
        "./storage-server".to_string(),
    );

    if let Err(_e) = photo_server.start() {
        println!("Photo server encountered an error");
    }
}
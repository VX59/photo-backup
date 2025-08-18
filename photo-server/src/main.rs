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

    pub fn start(&self) -> std::io::Result<()> {
        let listener = TcpListener::bind("0.0.0.0:8080")?;
        println!("Photo server listening on {}", self.address);
        
        for stream in listener.incoming() {
            let mut stream = stream.expect("Failed to accept connection");
            println!("New connection: {}", stream.peer_addr().expect("Failed to get peer address"));

            let response = Response {
                status_code: 200,
                status_message: "OK".to_string(),
                body: format!("connected to photo server @ {}", self.name).as_bytes().to_vec(),
            };

            stream.write_all(response.body.as_slice()).expect("Failed to write to stream");

            // read the repository path from client
            let mut repo_path_buffer = vec![0u8; 256];
            let n = stream.read(&mut repo_path_buffer).expect("Failed to read from stream");
            let repo_path_str = String::from_utf8_lossy(&repo_path_buffer[..n]);
            let repo_path = Path::new(repo_path_str.as_ref());
            println!("Server repository path: {}", repo_path_str);

            let mut response_body = format!("OK Repository path : '{}'", repo_path_str).as_bytes().to_vec();
            if repo_path.exists() == false {
                response_body = format!("Repository path does not exist on server: {}", repo_path_str).as_bytes().to_vec();
            }

            let response  = Response {
                status_code: 200,
                status_message: "OK".to_string(),
                body: response_body,
            };

            stream.write_all(response.body.as_slice()).expect("Failed to write to stream");
            stream.flush()?;

            if repo_path.exists() == false {
                println!("Closing connection due to invalid repository path.");
                drop(stream);
                continue;
            }

            self.photo_upload_handler(stream);
        }
        
        Ok(())
    }

    fn photo_upload_handler(&self, mut stream: TcpStream) {
        loop {
            let mut reader = BufReader::new(&stream);
            let name = get().unwrap_or_default().to_string_lossy().to_string();
            match upload_image(&mut reader) {
                Ok(file_name) => {
                    let response = format!("{} received {}: HTTP/1.1 200 OK\r\n", name, file_name);
                    stream.write_all(response.as_bytes()).expect("Failed to write to stream");
                }
                Err(e) => {
                    println!("Connection closed or error: {:?}", e);
                    break;
                }
            }
        }
    }
}

fn main() {
    let name = get().unwrap_or_default().to_string_lossy().to_string();
    let photo_server = PhotoServer::new(
        name.clone(),
        format!("{}:{}", name, "8080"),
        "./storage-server".to_string(),
    );

    photo_server.start().expect("Photo server encountered an error");
}

fn upload_image(reader:&mut BufReader<&TcpStream>) ->std::io::Result<String> {
    let mut header_length_buffer = [0u8; 4];
    // Read the request line

    match reader.read_exact(&mut header_length_buffer) {
        Ok(_) => {},
        Err(e) => {
            println!("Failed to read from stream: {}", e);
            return Err(e);
        }
    }
    let header_length = u32::from_be_bytes(header_length_buffer);
    println!("Header length: {}", header_length);
    let mut header_bytes = vec![0u8; header_length as usize];

    reader.read_exact(&mut header_bytes).expect("Failed to read image header from stream");

    let (file_header, _): (FileHeader, _) = bincode::decode_from_slice(&header_bytes, config::standard())
        .expect("Failed to deserialize file header");

    let mut image_bytes = vec![0u8; file_header.file_size as usize];
    println!("Receiving file: {} ({} bytes)", file_header.file_name, file_header.file_size);
    reader.read_exact(&mut image_bytes).expect("Failed to read image header from stream");

    let image = image::load_from_memory(&image_bytes).expect("failed to decode image");
    image.save(format!("./storage-server/{}", file_header.file_name))
        .expect("Failed to save image");

    Ok(file_header.file_name)
}
use std::{
    io::{prelude::*}, net::{TcpListener, TcpStream}, thread::{JoinHandle},
    path::Path, sync::{Arc, atomic}
};
use::bincode::{config};
use shared::{send_response, Response, Tree, FileHeader};

pub fn initiate_file_streaming_server(repo_name:String, storage_directory: String, listener:TcpListener, stop_flag:Arc<atomic::AtomicBool>) -> anyhow::Result<JoinHandle<()>>{   
    match listener.accept() {
        Ok((mut file_stream, socket_addr)) => {
            
            let response: Response = Response {
                status_code:shared::ResponseCodes::OK,
                status_message:"OK".to_string(),
                body: format!("{} Connected to repository {}", socket_addr, repo_name).as_bytes().to_vec(),
            };

            send_response(response, &mut file_stream)?;

            // move the file stream to its own thread
            return Ok(std::thread::spawn(move || {
                println!("file stream thread initiated");
                
                let mut file_stream_server = FileStreamServer::new(repo_name, storage_directory, file_stream, stop_flag);
                let repo_path = Path::new(&file_stream_server.storage_directory).join(&file_stream_server.repo_name);

                file_stream_server.run(repo_path.as_path());
            }));
        },
        Err(e) => return Err(anyhow::anyhow!(e)),
    }
}

struct FileStreamServer {
    repo_name: String,
    storage_directory: String,
    stream:TcpStream,
    stop_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
    tree:Tree,
}

impl FileStreamServer {
    pub fn new(repo_name:String,storage_directory: String, stream:TcpStream, stop_flag:std::sync::Arc<std::sync::atomic::AtomicBool>) -> Self{
        FileStreamServer {
            repo_name:repo_name.clone(),
            storage_directory,
            stream,
            stop_flag,
            tree: Tree::load_from_file(&("trees".to_string() + "/" + &repo_name + ".tree").to_string()),
        }
    }

    pub fn run(&mut self, file_dest: &Path) {
        while !self.stop_flag.load(std::sync::atomic::Ordering::Relaxed) {
            match self.upload_file(file_dest) {
                Ok(file_name) => {
                    let response = Response {
                        status_code:shared::ResponseCodes::OK,
                        status_message: "OK".to_string(),
                        body: format!("received {:?}", file_name).as_bytes().to_vec(),
                    };
                    
                    if let Err(e) = send_response(response, &mut self.stream) {
                        println!("{}", e);
                        break;
                    }
                }
                Err(e) => {
                    println!("Connection closed or error: {}", e);
                    break;
                }
            }
        }
    }

    fn upload_file(&mut self, file_dest: &Path) -> anyhow::Result<String, anyhow::Error> {
        let mut header_length_buffer = [0u8; 4];
        // Read the request line

        self.stream.read_exact(&mut header_length_buffer)?;

        let header_length = u32::from_be_bytes(header_length_buffer);
        println!("Header length: {}", header_length);
        let mut header_bytes = vec![0u8; header_length as usize];

        self.stream.read_exact(&mut header_bytes)?;
        
        let (file_header, _): (FileHeader, _) = bincode::decode_from_slice(&header_bytes, config::standard())?;

        let mut image_bytes = vec![0u8; file_header.file_size as usize];

        self.stream.read_exact(&mut image_bytes)?;


        println!("{} is the file dest", file_dest.to_str().unwrap().to_string());
        let image_loc = file_dest.join(file_header.relative_path);
        let image_path = image_loc.join(file_header.file_name);

        self.tree.add_history( format!("+{}", image_path.to_str().unwrap().to_string()));
        self.tree.apply_history(self.tree.version);
        self.tree.save_to_file(&self.tree.path);

        println!("Receiving file: {} ({} bytes)", image_path.to_str().unwrap().to_string(), file_header.file_size);

        if std::path::Path::new(&image_loc).exists() == false {
            std::fs::create_dir_all(&image_loc)?;
        }

        std::fs::write(&image_path, &image_bytes)?;
        Ok(image_path.to_string_lossy().to_string())

    }
}
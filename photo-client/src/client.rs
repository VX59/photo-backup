use std::sync::mpsc::channel;
use std::time::Duration;
use bincode::config;
use image::ImageReader;
use chrono;
use bincode::encode_into_slice;
use shared::send_request;
use shared::RequestTypes;
use std::io;
use std::io::prelude::*;
use std::io::Cursor;
use std::net::TcpStream;
use std::path::Path;
use shared::FileHeader;
use notify::{Watcher,RecommendedWatcher, RecursiveMode, EventKind};
use std::path::PathBuf;
use std::sync::mpsc;

use shared::{read_response, Request};
use shared::Commands;
use crate::app::Config;

pub struct ImageClient {
    pub app_tx: mpsc::Sender<Commands>,
    rx: mpsc::Receiver<Commands>,
    stop_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
    config: Config,
    stream: Option<TcpStream>,
}

impl ImageClient {
    pub fn new(app_tx: mpsc::Sender<Commands>,rx:mpsc::Receiver<Commands>,stop_flag: std::sync::Arc<std::sync::atomic::AtomicBool>) -> Self {
        let config_path = PathBuf::from("photo-client-config.json");
        let config: Config = Config::load_from_file(config_path.to_str().unwrap());

        ImageClient {
            app_tx,
            rx, 
            stop_flag,
            config,
            stream: None,
        }
    }

    pub fn connect(&mut self) -> io::Result<()> {
        match TcpStream::connect(self.config.server_address.as_str()) {
            Ok(s) => {
                self.stream = Some(s);

                if let Some(stream) = self.stream.as_mut() {
                    // Handle the connection response
                    
                    let connection_response = read_response(stream)?;
                    let connection_response_message = String::from_utf8_lossy(&connection_response.body);
                    let connection_response_code = connection_response.status_code;

                    let _ = self.app_tx.send(Commands::Log(format!("{} | [ {} ]",connection_response_code,  connection_response_message))).unwrap();

                    // Send storage directory path to server

                    let request = Request {
                        request_type: RequestTypes::SetStoragePath,
                        body: self.config.server_storage_directory.as_bytes().to_vec(),
                    };

                    send_request(request, stream)?;

                    // Handle the storage directory response

                    let response = read_response(stream)?;
                    let response_message = String::from_utf8_lossy(&response.body);
                    let response_code = response.status_code;
                    
                    if response_code == 200 {
                        // send the repo list to the app
                        let available_repositories:Vec<String> = serde_json::from_slice(&response.body)?;
                        self.app_tx.send(Commands::PostRepos(available_repositories)).unwrap();
                    }

                    self.app_tx.send(Commands::Log(format!("{} | [ {} ]", response_code, response_message))).unwrap();
                }
                
                // listen to the app for commands
                while !self.stop_flag.load(std::sync::atomic::Ordering::Relaxed) {
                    match self.rx.try_recv() {
                        Ok(new_command) => {
                            match new_command {
                                Commands::CreateRepo(msg) => {
                                    self.create_repository(msg.to_string())?;
                                }
                                Commands::StartStream(repo) => {

                                    // request a file streaming channel - the server will open another port
                                    let file_stream = self.setup_streaming_channel(repo.to_string())?;
                                    // run the file streaming channel on a seperate thread
                                }
                                _ => {},
                            }
                        }
                        Err(std::sync::mpsc::TryRecvError::Empty) => { /* just continue */ },
                        Err(e) => return Err(std::io::Error::new(std::io::ErrorKind::Other, e)),
                    }
                }
                
                if let Some(stream) = self.stream.as_mut() {
                    stream.shutdown(std::net::Shutdown::Both).ok();
                }
                self.stream.take();
                self.app_tx.send(Commands::Log("Photo Client stopped.".to_string())).ok();

            },
            Err(e) => self.app_tx.send(Commands::Log(format!("error {}",e).to_string())).unwrap(),
        }
        Ok(())
    }

    pub fn setup_streaming_channel(&mut self, repo:String) -> io::Result<TcpStream> {
        if let Some(stream) = self.stream.as_mut() {
            let request = Request {
                request_type: RequestTypes::StartStream,
                body: repo.as_bytes().to_vec(), // implement security stuff here
            };

            send_request(request, stream)?;

            let response = read_response(stream)?;
            let file_stream_address = String::from_utf8_lossy(&response.body).to_string();
            
            let file_stream = TcpStream::connect(file_stream_address.as_str())?;

            let _ = self.app_tx.send(Commands::Log(format!("{} | [ {} ]", response.status_code, response.status_message))).unwrap();
            // handshake to confirm connection
            
            return Ok(file_stream);
        }
        Err(std::io::Error::new(std::io::ErrorKind::NotConnected,"Main stream is not connected"))
    }

    pub fn create_repository(&mut self, repo_name:String) -> io::Result<()> {
        
        if let Some(stream) = self.stream.as_mut() {
            let request = Request {
                request_type: RequestTypes::CreateRepo,
                body: repo_name.as_bytes().to_vec(),
            };

            send_request(request, stream)?;

            let response = read_response(stream)?;
            let response_message = String::from_utf8_lossy(&response.body);
            let _ = self.app_tx.send(Commands::Log(format!("{} | [ {} ]", response.status_code, response_message))).unwrap();
        }
        Ok(())
    }

    pub fn run_streaming_channel(&mut self) -> io::Result<()> {
        let (tx, rx) = channel();
        let mut watcher = match RecommendedWatcher::new(move |res| tx.send(res).unwrap(), notify::Config::default())
            {
        Ok(w) => w,
        Err(e) => {
            eprintln!("Failed to create file watcher: {}", e);
            return Err(io::Error::new(io::ErrorKind::Other, "Failed to create file watcher"));
            }
        };

        if let Err(e) = watcher.watch(Path::new(self.config.watch_directory.as_str()), RecursiveMode::Recursive) {
            eprintln!("Failed to watch directory: {}", e);
            return Err(io::Error::new(io::ErrorKind::Other, "Failed to watch directory"));
        }

        while !self.stop_flag.load(std::sync::atomic::Ordering::Relaxed) {
            match rx.try_recv() {
                Ok(event) => {
                    let new_event = match event {
                        Ok(ev) => ev,
                        Err(e) => {
                            eprintln!("Watch error: {:?}", e);
                            continue;
                        }
                    };
                    if let EventKind::Create(notify::event::CreateKind::File) = new_event.kind {

                        if let Some(ref mut stream) = self.stream {
                            for path in new_event.paths {
                                if let Err(e) = upload_image(path, "...".to_string(),stream, &self.app_tx, self.stop_flag.clone()) {
                                    eprintln!("Failed to send image: {}", e);
                                    break;
                                };
                            }
                        } else {
                            eprintln!("No active stream to send images.");
                        }
                    }
                },
                Err(mpsc::TryRecvError::Empty) => {
                    std::thread::sleep(Duration::from_secs(1));
                    continue;
                },
                Err(mpsc::TryRecvError::Disconnected) => {
                    // Channel disconnected
                    break;
                }
            }
        }

        if let Some(stream) = self.stream.as_mut() {
            stream.shutdown(std::net::Shutdown::Both).ok();
        }
        self.stream.take();
        self.app_tx.send(Commands::Log("Photo Client stopped.".to_string())).ok();
        

        Ok(())
    }

}

fn upload_image(local_path:PathBuf, dest_path:String, stream:&mut TcpStream, tx: & mpsc::Sender<Commands>, stop_flag: std::sync::Arc<std::sync::atomic::AtomicBool>) -> std::io::Result<()> { 
    if !local_path.exists() {
        return Err(io::Error::new(io::ErrorKind::NotFound, "File not found"));
    }

    let mut last_size = 0;
    loop {
        if stop_flag.load(std::sync::atomic::Ordering::Relaxed) {
            return Ok(())
        }
        let metadata = std::fs::metadata(&local_path)?;
        let current_size = metadata.len();
        if current_size == last_size {
            break;
        }
        last_size = current_size;
    }

    let file_name = local_path.file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Invalid file name"))?;

    let file_ext = local_path.extension()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown");

    let file_datetime = local_path.metadata()?.created()?;

    let image = ImageReader::open(local_path.clone())?.decode().expect("unable to decode image");
    let mut image_bytes: Vec<u8> = Vec::new();

    if file_ext == "png" {
        image.write_to(&mut Cursor::new(&mut image_bytes), image::ImageFormat::Png)
            .expect("Failed to write PNG image");
    } else if file_ext == "jpg" || file_ext == "jpeg" {
        image.write_to(&mut Cursor::new(&mut image_bytes), image::ImageFormat::Jpeg)
            .expect("Failed to write JPEG image");
    } else {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Unsupported image format"));
    }

    let file_header = FileHeader {
        file_name: file_name.to_string(), 
        file_size: image_bytes.len() as u64,
        file_ext: file_ext.to_string(),
        file_datetime: file_datetime,
        file_dest: dest_path,
    };

    let mut header_bytes = vec![0u8; 1024];
    let header_size = encode_into_slice(&file_header, &mut header_bytes[..], config::standard())
    .expect("Failed to serialize file header") as u32;

    header_bytes.truncate(header_size as usize);
    println!("Header size: {}", header_size);
        
    println!("Image size: {}", image_bytes.len());

    // write to the stream
    stream.write_all(&header_size.to_be_bytes())?;
    stream.write_all(&header_bytes)?;
    stream.write_all(&image_bytes)?;

    println!("Sent file: {} ({} bytes)", file_header.file_name, file_header.file_size);

    let response_buffer = {
        let mut buffer = vec![0u8; 256];
        let n = stream.read(&mut buffer)?;
        buffer[..n].to_vec()
    };
    let response_message = String::from_utf8_lossy(&response_buffer);
    let _ = tx.send(Commands::Log(format!("{}", response_message))).unwrap();
    stream.flush()?;

    // log the timestamp of the most recent backup
    let timestamp = chrono::DateTime::<chrono::Local>::from(file_datetime);
    let _ = tx.send(Commands::Log(format!("Backed up '{}' at {}", file_header.file_name, timestamp.format("%Y-%m-%d %H:%M:%S")))).unwrap();

    let mut f = std::fs::File::create("./last_backup.txt")?;
    f.write_all(timestamp.to_string().as_bytes())?;

    Ok(())
}
use std::sync::mpsc::channel;
use std::time::Duration;
use bincode::config;
use image::ImageReader;
use bincode::encode_into_slice;
use std::io;
use std::io::prelude::*;
use std::io::Cursor;
use std::net::TcpStream;
use std::path::Path;
use shared::FileHeader;
use notify::{Watcher,RecommendedWatcher, RecursiveMode, EventKind};
use std::path::PathBuf;
use std::sync::mpsc;

use crate::app::Config;

pub struct ImageClient {
    log_tx: mpsc::Sender<String>,
    stop_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
    config: Config,
    stream: Option<TcpStream>,
}

impl ImageClient {
    pub fn new(log_tx: mpsc::Sender<String>, stop_flag: std::sync::Arc<std::sync::atomic::AtomicBool>) -> Self {
        let config_path = PathBuf::from("photo-client-config.json");
        let config: Config = Config::load_from_file(config_path.to_str().unwrap());

        ImageClient {
            log_tx,
            stop_flag,
            config,
            stream: None,
        }
    }

    pub fn connect(&mut self) -> io::Result<()> {
        if let Ok(s) = TcpStream::connect(self.config.server_address.as_str()) {
            self.stream = Some(s);
            let response_buffer = {
                let mut buffer = vec![0u8; 256];
                let n = match self.stream.as_mut().unwrap().read(&mut buffer) {
                    Ok(n) => n,
                    Err(e) => {
                        eprintln!("Failed to read from server: {}", e);
                        return Err(e);
                    }
                };
                buffer[..n].to_vec()
            };

            let response_message = String::from_utf8_lossy(&response_buffer);
            let _ = self.log_tx.send(format!("Connected to server: {}", response_message)).unwrap();
            let _ = self.log_tx.send(format!("initiating image client to monitor '{}' directory for new images...", self.config.watch_directory.as_str())).unwrap();
        };


        Ok(())
    }

    pub fn run(&mut self) -> io::Result<()> {
        self.connect()?;
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
            match rx.recv_timeout(Duration::from_millis(1000)) {
                Ok(event) => {
                    let new_event = match event {
                        Ok(ev) => ev,
                        Err(e) => {
                            eprintln!("Watch error: {:?}", e);
                            continue;   
                        }
                    };
                    if let EventKind::Create(notify::event::CreateKind::File) = new_event.kind {
                        for path in new_event.paths {
                            if let Err(e) = write_image(path,self.stream.as_mut().unwrap(),&self.log_tx, self.stop_flag.clone()) {
                                eprintln!("Failed to send image: {}", e);
                                break;
                            };
                        }
                    }
                },
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    continue;
                },
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    // Channel disconnected
                    break;
                }
            }
        }

        if let Some(stream) = self.stream.as_mut() {
            stream.shutdown(std::net::Shutdown::Both).ok();
        }
        self.stream.take();
        self.log_tx.send("Photo Client stopped.".to_string()).ok();

        Ok(())
    }

}

fn write_image(path:PathBuf, stream:&mut TcpStream, tx: & mpsc::Sender<String>, stop_flag: std::sync::Arc<std::sync::atomic::AtomicBool>) -> std::io::Result<()> { 
    if !path.exists() {
        return Err(io::Error::new(io::ErrorKind::NotFound, "File not found"));
    }

    let mut last_size = 0;
    loop {
        if stop_flag.load(std::sync::atomic::Ordering::Relaxed) {
            return Ok(())
        }
        let metadata = std::fs::metadata(&path)?;
        let current_size = metadata.len();
        if current_size == last_size {
            break;
        }
        last_size = current_size;
        std::thread::sleep(std::time::Duration::from_millis(250));
    }

    let file_name = path.file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Invalid file name"))?;

    let file_ext = path.extension()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown");

    let file_datetime = path.metadata()?.created()?;

    let image = ImageReader::open(path.clone())?.decode().expect("unable to decode image");
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
    let _ = tx.send(format!("{}", response_message)).unwrap();
    stream.flush()?;

    Ok(())
}
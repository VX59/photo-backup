use std::path::Path;
use shared::FileHeader;
use notify::{Watcher,RecommendedWatcher, RecursiveMode, EventKind};
use std::io::prelude::*;
use std::io::Cursor;
use std::sync::mpsc::channel;
use std::time::Duration;
use bincode::config;
use image::ImageReader;
use chrono;
use bincode::encode_into_slice;
use std::io;
use std::net::TcpStream;
use std::path::PathBuf;
use crate::app::{Commands, Config};
use shared::{read_response};
use std::sync::mpsc;

pub struct FileStreamClient {
    stream:TcpStream,
    config:Config,
    pub app_tx:mpsc::Sender<Commands>,
    stop_flag:std::sync::Arc<std::sync::atomic::AtomicBool>
}

impl FileStreamClient {
    pub fn new(stream:TcpStream, config:Config, app_tx:mpsc::Sender<Commands>, stop_flag: std::sync::Arc<std::sync::atomic::AtomicBool>) -> Self {
        FileStreamClient { 
            stream,
            config,
            app_tx,
            stop_flag
        }
    }

    pub fn run(&mut self) -> io::Result<()> {
        let (wtx, wrx) = channel();
        let mut watcher = match RecommendedWatcher::new(move |res| wtx.send(res).unwrap(), notify::Config::default())
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
            match wrx.try_recv() {
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
                            if let Err(e) = self.upload_file(path) {
                                eprintln!("Failed to send image: {}", e);
                                break;
                            };
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

        self.stream.shutdown(std::net::Shutdown::Both).ok();
        self.app_tx.send(Commands::Log("Repo client stopped.".to_string())).ok();

        Ok(())
    }

    fn upload_file(&mut self, local_path:PathBuf) -> std::io::Result<()> { 
        if !local_path.exists() {
            return Err(io::Error::new(io::ErrorKind::NotFound, "File not found"));
        }

        let mut last_size = 0;
        loop {
            if self.stop_flag.load(std::sync::atomic::Ordering::Relaxed) {
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
        };

        let mut header_bytes = vec![0u8; 1024];
        let header_size = encode_into_slice(&file_header, &mut header_bytes[..], config::standard())
        .expect("Failed to serialize file header") as u32;

        header_bytes.truncate(header_size as usize);
        println!("Header size: {}", header_size);
            
        println!("Image size: {}", image_bytes.len());

        // write to the stream
        self.stream.write_all(&header_size.to_be_bytes())?;
        self.stream.write_all(&header_bytes)?;
        self.stream.write_all(&image_bytes)?;

        println!("Sent file: {} ({} bytes)", file_header.file_name, file_header.file_size);

        let response = read_response(&mut self.stream)?;
        let response_message = String::from_utf8_lossy(&response.body);
        self.app_tx.send(Commands::Log(format!("{} | [ {} ]", response.status_code, response_message))).unwrap();

        // log the timestamp of the most recent backup
        let timestamp = chrono::DateTime::<chrono::Local>::from(file_datetime);

        let mut f = std::fs::File::create("./last_backup.txt")?;
        f.write_all(timestamp.to_string().as_bytes())?;

        Ok(())
    }
}
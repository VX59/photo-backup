use std::{path::{Path,PathBuf}, io::{prelude::*, Cursor}, sync::{mpsc,Arc,atomic},
                time::Duration, net::TcpStream,};
use bincode::{config, encode_into_slice};
use image::ImageReader;
use notify::{Watcher,RecommendedWatcher, RecursiveMode, EventKind};
use shared::{read_response, Response, Log, Notify, FileHeader};
use crate::app::{Commands};

pub struct FileStreamClient {
    stream:TcpStream,
    watch_directory:String,
    pub app_tx:mpsc::Sender<Commands>,
    stop_flag:Arc<atomic::AtomicBool>,
    track_modifications:bool,
}

impl FileStreamClient {
    pub fn new(stream:TcpStream, watch_directory:String, app_tx:mpsc::Sender<Commands>, stop_flag: Arc<atomic::AtomicBool>, track_modifications:bool) -> Self {
        FileStreamClient { 
            stream,
            watch_directory,
            app_tx,
            stop_flag,
            track_modifications,
        }
    }

    pub fn run(&mut self) -> anyhow::Result<()> {
        let (wtx, wrx) = mpsc::channel();
        let mut watcher = match RecommendedWatcher::new(move |res| 
            match wtx.send(res) {
                Ok(res) => res,
                Err(_) => {},

            }, notify::Config::default())
            {
        Ok(w) => w,
        Err(e) => {
            eprintln!("Failed to create file watcher: {}", e);
            return Err(anyhow::anyhow!("Failed to create file watcher"));
            }
        };

        if let Err(e) = watcher.watch(Path::new(&self.watch_directory), RecursiveMode::Recursive) {
            return Err(anyhow::anyhow!("Failed to watch directory: {} {}",self.watch_directory, e));
        }


        while !self.stop_flag.load(atomic::Ordering::Relaxed) {
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
                        for path in new_event.paths.clone() {
                            if let Err(e) = self.upload_file(path) {
                                eprintln!("Failed to send image: {}", e);
                                break;
                            };
                        }
                    }
                    if self.track_modifications {
                        if let EventKind::Modify(notify::event::ModifyKind::Any) = new_event.kind {
                            for path in new_event.paths {
                                if let Err(e) = self.upload_file(path) {
                                    eprintln!("Failed to send image: {}", e);
                                    break;
                                };
                            }
                        }
                    }
                },
                Err(mpsc::TryRecvError::Empty) => {
                    std::thread::sleep(Duration::from_secs(1));
                    continue;
                },
                Err(mpsc::TryRecvError::Disconnected) => {
                    break;
                }
            }
        }

        self.stream.shutdown(std::net::Shutdown::Both).ok();
        let message = "Streaming client stopped.".to_string();
        self.app_tx.send(Commands::Log(message.clone()))?;
        self.app_tx.send(Commands::Notify(message.clone()))?;

        Ok(())
    }

    fn upload_file(&mut self, local_path:PathBuf) -> anyhow::Result<()> { 
        if !local_path.exists() {
            return Err(anyhow::anyhow!("File not found"));
        }

        let mut last_size = 0;
        loop {
            if self.stop_flag.load(atomic::Ordering::Relaxed) {
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
            .ok_or_else(|| anyhow::anyhow!("Invalid file name"))?;

        let file_ext = local_path.extension()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown");
        let file_datetime = local_path.metadata()?.created()?;
        let mut image_bytes: Vec<u8> = Vec::new();
        if ["png,jpg,jpeg,gif"].contains(&file_ext) {
            let image = ImageReader::open(local_path.clone())?.decode().expect("unable to decode image");

            match file_ext {
                "png" => {
                image.write_to(&mut Cursor::new(&mut image_bytes), image::ImageFormat::Png)
                    .expect("Failed to write PNG image");
                },
                "jpg" | "jpeg" => {
                    image.write_to(&mut Cursor::new(&mut image_bytes), image::ImageFormat::Jpeg)
                    .expect("Failed to write JPEG image");
                },

                "gif" => {
                    image.write_to(&mut Cursor::new(&mut image_bytes), image::ImageFormat::Gif)
                    .expect("Failed to write GIF image");
                }
                _ => return Err(anyhow::anyhow!("Unsupported image format")), // maybe fix this
            }
        } else {
            image_bytes = std::fs::read(local_path.clone())?;
        }

        let repo_root = Path::new(&self.watch_directory);
        let relative_path = match local_path.strip_prefix(repo_root) {
            Ok(path) => {
                let parent_path = match path.parent().unwrap_or_else(|| Path::new("")).to_str() {
                    Some(p) => p.to_string(),
                    None => return Err(anyhow::anyhow!("Failed to get relative path")),
                };
                parent_path
            },
            Err(_) => return Err(anyhow::anyhow!("File is outside of repository root")),
        };
        let file_header = FileHeader {
            file_name: file_name.to_string(), 
            relative_path: relative_path,
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

        println!("Sent file: {} ({} bytes)",file_header.file_name, file_header.file_size);

        let response = read_response(&mut self.stream)?;
        self.log_response(&response)?;
        self.notify_app(&response)?;

        Ok(())
    }
}

impl Notify for FileStreamClient {
    fn notify_app(&self, response:&Response) -> anyhow::Result<()> {   
        let response_message = String::from_utf8_lossy(&response.body);
        let _ = self.app_tx.send(Commands::Notify(format!("{}", response_message)))?;
        Ok(())
    }
}

impl Log for FileStreamClient {
    fn log_response(&self, response:&Response) -> anyhow::Result<()> {   
        let response_message = String::from_utf8_lossy(&response.body);
        let _ = self.app_tx.send(Commands::Log(format!("{} | [ {} ]", response.status_code, response_message)))?;
        Ok(())
    }

}
use std::io;
use std::io::prelude::*;
use std::io::Cursor;
use std::net::TcpStream;
use std::path::Path;
use std::path::PathBuf;
use std::sync::mpsc::channel;
use bincode::config;
use image::ImageReader;
use bincode::encode_into_slice;

use notify::Config;
use shared::FileHeader;
use notify::{Watcher,RecommendedWatcher, RecursiveMode, EventKind};


fn main() -> std::io::Result<()> {

    image_client();
Ok(())
}

pub extern "C"
fn image_client() -> i32{

    let server_address = "jacob-ubuntu:8080";
    let mut stream= match TcpStream::connect(server_address) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to connect to server {}: {}", server_address, e);
            return -1;
        }
    };

    let response_buffer = {
        let mut buffer = vec![0u8; 256];
        let n = match stream.read(&mut buffer) {
            Ok(n) => n,
            Err(e) => {
                eprintln!("Failed to read from server: {}", e);
                return -2;
            }
        };
        buffer[..n].to_vec()
    };

    let response_message = String::from_utf8_lossy(&response_buffer);
    println!("Received from server: {}", response_message);
    println!("initiating image client to monitor 'storage-client' directory for new images...");

    let (tx, rx) = channel();
    let mut watcher = match RecommendedWatcher::new(move |res| tx.send(res).unwrap(), Config::default())
        {
    Ok(w) => w,
    Err(e) => {
        eprintln!("Failed to create file watcher: {}", e);
        return -3;
        }
    };

    if let Err(e) = watcher.watch(Path::new("storage-client"), RecursiveMode::Recursive) {
        eprintln!("Failed to watch directory: {}", e);
        return -4;
    }

    loop {
        match rx.recv() {
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
                        if let Err(e) = write_image(path,&mut stream) {
                            eprintln!("Failed to send image: {}", e);
                            break;
                        };
                    }
                }
            },
            Err(e) => {
                eprintln!("Watch error: {:?}", e);
                return -5;
            }
        }
    } 
    0
}

fn write_image(path:PathBuf, stream:&mut TcpStream) -> std::io::Result<()> { 
    
    if !path.exists() {
        return Err(io::Error::new(io::ErrorKind::NotFound, "File not found"));
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

    Ok(())
}
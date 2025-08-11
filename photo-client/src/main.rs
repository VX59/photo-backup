use std::io;
use std::io::prelude::*;
use std::io::Cursor;
use std::net::TcpStream;
use std::fs::DirEntry;
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

    let (tx, rx) = channel();
    let mut watcher = RecommendedWatcher::new(move |res| tx.send(res).unwrap(), Config::default())
        .expect("Failed to create watcher");

    watcher.watch(Path::new("storage-client"), RecursiveMode::Recursive)
        .expect("Failed to watch directory");
    let server_address = "jacob-ubuntu:8080";
    let mut stream = TcpStream::connect(server_address).expect("Could not connect to server");

    loop {
        match rx.recv() {
            Ok(event) => {
                let new_event = event.unwrap();
                if let EventKind::Create(notify::event::CreateKind::File) = new_event.kind {
                    for path in new_event.paths {
                        write_image(path, &mut stream)?;
                    }
                }

            },
            Err(e) => eprintln!("Watch error: {:?}", e),
        }
    }
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
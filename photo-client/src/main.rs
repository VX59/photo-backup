use std::io;
use std::io::prelude::*;
use std::io::Cursor;
use std::net::TcpStream;
use std::fs::DirEntry;
use bincode::config;
use image::ImageReader;
use bincode::encode_into_slice;

use shared::FileHeader;

fn main() -> std::io::Result<()> {
    let server_address = "jacob-laptop:8080";
    let mut stream = TcpStream::connect(server_address).expect("Could not connect to server");

    let file_num = std::fs::read_dir("storage-client")?
        .count() as u32;

    // write number of files to the server
    println!("Number of files: {}", file_num);
    stream.write_all(&file_num.to_be_bytes())?;

    for file in std::fs::read_dir("storage-client")? {
        let entry = file?;
        write_image(entry, &mut stream)?;
    }
    println!("All images sent to server");

    let buffer: &mut [u8; 1024] = &mut [0; 1024];
    stream.read(buffer)
          .expect("Failed to read from stream");
    println!("Received from server: {}", buffer.iter()
        .take_while(|&&x| x != 0)
        .map(|&x| x as char)
        .collect::<String>());
    Ok(())
}

fn write_image(file:DirEntry, stream:&mut TcpStream) -> std::io::Result<()> { 
    let path = file.path();
    
    if !path.exists() {
        return Err(io::Error::new(io::ErrorKind::NotFound, "File not found"));
    }

    let mut file_header = FileHeader {
        file_name: path.file_name().and_then(|s| s.to_str()).unwrap_or("unknown").to_string(),
        file_size: 0,
        file_ext: path.extension().and_then(|s| s.to_str()).unwrap_or("unknown").to_string(),
    };

    let mut header_bytes = vec![0u8; 1024];
    let header_size = encode_into_slice(&file_header, &mut header_bytes[..], config::standard())
    .expect("Failed to serialize file header") as u32;

    header_bytes.truncate(header_size as usize);

    let image = ImageReader::open(path)?.decode().expect("unable to decode image");
    let mut bytes: Vec<u8> = Vec::new();
    
    image.write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Jpeg).expect("failed to write image to buffer");
    file_header.file_size = bytes.len() as u64;
    
    println!("Prepared file: {} ({} bytes)", file_header.file_name, file_header.file_size);
    // write to the stream
    stream.write_all(&header_size.to_be_bytes())?;
    stream.write_all(&header_bytes)?;
    stream.write_all(&bytes)?;

    println!("Sent file: {} ({} bytes)", file_header.file_name, file_header.file_size);

    Ok(())
}
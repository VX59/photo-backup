use std::io;
use std::io::prelude::*;
use std::io::Cursor;
use std::net::TcpStream;
use std::path::PathBuf;
use image::ImageReader;

fn main() -> std::io::Result<()> {
    let server_address = "jacob-laptop:8080";
    let mut stream = TcpStream::connect(server_address).expect("Could not connect to server");

    let file_num = std::fs::read_dir("storage-client")?
        .count();

    // write number of files to the server
    println!("Number of files: {}", file_num);
    stream.write_all(&file_num.to_be_bytes())?;

    for file in std::fs::read_dir("storage-client")? {
        let entry = file?;
        write_image(entry.path(), &mut stream)?;
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

fn write_image(path:PathBuf, stream:&mut TcpStream) -> std::io::Result<()> {    
    if !path.exists() {
        return Err(io::Error::new(io::ErrorKind::NotFound, "File not found"));
    }

    let image = ImageReader::open(path)?.decode().expect("unable to decode image");
    let mut bytes: Vec<u8> = Vec::new();
    
    image.write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Jpeg).expect("failed to write image to buffer");
    let length = bytes.len() as u64;
    // write to the stream
    stream.write_all(&length.to_be_bytes())?;
    stream.write_all(&bytes)?;
    Ok(())
}
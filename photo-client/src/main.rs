use std::io::prelude::*;
use std::io::Cursor;
use std::net::TcpStream;
use image::ImageReader;

fn main() -> std::io::Result<()> {
    let server_address = "jacob-laptop:8080";
    let mut stream = TcpStream::connect(server_address).expect("Could not connect to server");

    let image = ImageReader::open("./client-storage/test-image.jpg")?.decode().expect("unable to decode image");
    let mut bytes: Vec<u8> = Vec::new();

    image.write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Jpeg).expect("failed to write image to buffer");
    
    let length = bytes.len() as u64;

    // write to the stream
    stream.write_all(&length.to_be_bytes())?;
    stream.write_all(&bytes)?;

    let buffer: &mut [u8; 1024] = &mut [0; 1024];
    stream.read(buffer)
          .expect("Failed to read from stream");
    println!("Received from server: {}", buffer.iter()
        .take_while(|&&x| x != 0)
        .map(|&x| x as char)
        .collect::<String>());
    Ok(())
}
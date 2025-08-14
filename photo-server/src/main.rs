use std::{
    io::{BufReader, prelude::*},
    net::{TcpListener, TcpStream},
};
use::bincode::{config};
use::shared::FileHeader;
use shared::Response;
use hostname::get;

fn main() {
    let listener = TcpListener::bind("0.0.0.0:8080").expect("Failed to bind to address");
    let name = get().unwrap_or_default().to_string_lossy().to_string();

    println!("Photo server '{}' listening on port 8080", name);

    for stream in listener.incoming() {
        let mut stream = stream.expect("Failed to accept connection");
        println!("New connection: {}", stream.peer_addr().expect("Failed to get peer address"));

        let response = Response {
            status_code: 200,
            status_message: "OK".to_string(),
            body: format!("connected to photo server @ {}", name).as_bytes().to_vec(),
        };

        stream.write_all(response.body.as_slice()).expect("Failed to write to stream");

        image_upload_handler(stream);
    }
}

fn image_upload_handler(mut stream: TcpStream){
    loop {
        let mut reader = BufReader::new(&stream);
        let name = get().unwrap_or_default().to_string_lossy().to_string();
        match upload_image(&mut reader) {
            Ok(file_name) => {
                let response = format!("{} received {}: HTTP/1.1 200 OK\r\n", name, file_name);
                stream.write_all(response.as_bytes()).expect("Failed to write to stream");
            }
            Err(e) => {
                println!("Connection closed or error: {:?}", e);
                break;
            }
        }
    }
}

fn upload_image(reader:&mut BufReader<&TcpStream>) ->std::io::Result<String> {
    let mut header_length_buffer = [0u8; 4];
    // Read the request line

    reader.read_exact(&mut header_length_buffer).expect("failed to read length");
    let header_length = u32::from_be_bytes(header_length_buffer);
    println!("Header length: {}", header_length);
    let mut header_bytes = vec![0u8; header_length as usize];

    reader.read_exact(&mut header_bytes).expect("Failed to read image header from stream");

    let (file_header, _): (FileHeader, _) = bincode::decode_from_slice(&header_bytes, config::standard())
        .expect("Failed to deserialize file header");

    let mut image_bytes = vec![0u8; file_header.file_size as usize];
    println!("Receiving file: {} ({} bytes)", file_header.file_name, file_header.file_size);
    reader.read_exact(&mut image_bytes).expect("Failed to read image header from stream");

    let image = image::load_from_memory(&image_bytes).expect("failed to decode image");
    image.save(format!("./storage-server/{}", file_header.file_name))
        .expect("Failed to save image");

    Ok(file_header.file_name)
}
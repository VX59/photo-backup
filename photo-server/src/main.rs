use std::{
    io::{BufReader, prelude::*},
    net::{TcpListener, TcpStream},
};

fn main() {
    let listener = TcpListener::bind("jacob-laptop:8080").expect("Failed to bind to address");
    println!("Server is running on jacob-laptop:8080");

    for stream in listener.incoming() {
        let stream = stream.expect("Failed to accept connection");
        println!("New connection: {}", stream.peer_addr().expect("Failed to get peer address"));

        handle_connection(stream);
    }
}

 fn handle_connection(mut stream: TcpStream) {
    let mut reader = BufReader::new(&stream);
    let mut length_buffer = [0u8; 8];
    // Read the request line

    reader.read_exact(&mut length_buffer).expect("failed to read length");
    let length = u64::from_be_bytes(length_buffer);

    let mut bytes = vec![0u8; length as usize];

    reader.read_exact(&mut bytes).expect("Failed to read image from stream");
    let image = image::load_from_memory(&bytes).expect("failed to decode image");
    image.save("./server-storage/test-image.jpg").expect("failed to save the image");

    let response = "HTTP/1.1 200 OK\r\nContent-Length: 13\r\n\r\n successfully uploaded image!";
    stream.write_all(response.as_bytes()).expect("Failed to write to stream");
 }
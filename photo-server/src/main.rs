use std::{
    io::{BufReader, prelude::*},
    net::{TcpListener, TcpStream},
};

fn main() {
    let listener = TcpListener::bind("jacob-ubuntu:8080").expect("Failed to bind to address");
    println!("Server is running on jacob-ubuntu:8080");

    for stream in listener.incoming() {
        let stream = stream.expect("Failed to accept connection");
        println!("New connection: {}", stream.peer_addr().expect("Failed to get peer address"));

        handle_connection(stream);
    }
}

 fn handle_connection(mut stream: TcpStream) {
    let mut reader = BufReader::new(&stream);
    let mut buffer: String = String::new();

    // Read the request line
    reader.read_line(&mut buffer).expect("Failed to read from stream");
    println!("Request: {}", buffer.trim());

    // Respond with a simple HTTP response
    let response = "HTTP/1.1 200 OK\r\nContent-Length: 13\r\n\r\nHello, world!";
    stream.write_all(response.as_bytes()).expect("Failed to write to stream");
 }
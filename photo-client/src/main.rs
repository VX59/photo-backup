use std::io::prelude::*;
use std::net::TcpStream;
fn main() -> std::io::Result<()> {
    let server_address = "jacob-ubuntu:8080";
    let mut stream = TcpStream::connect(server_address).expect("Could not connect to server");

    stream.write(b"Hello, server!\n")
          .expect("Failed to write to stream");

    let buffer: &mut [u8; 1024] = &mut [0; 1024];
    stream.read(buffer)
          .expect("Failed to read from stream");
    println!("Received from server: {}", buffer.iter()
        .take_while(|&&x| x != 0)
        .map(|&x| x as char)
        .collect::<String>());
    Ok(())
}
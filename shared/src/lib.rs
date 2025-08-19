use std::net::TcpStream;
use std::io::Read;
use std::io::Write;
use bincode::{Decode, Encode};
use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Encode, Decode)]
pub struct FileHeader {
    pub file_name: String,
    pub file_size: u64,
    pub file_ext: String,
    pub file_datetime: std::time::SystemTime,
    pub file_dest: String,
}

#[derive(Serialize, Deserialize)]
pub struct Response {
    pub status_code: u16,
    pub status_message: String,
    pub body: Vec<u8>,
}

pub fn read_response(stream:&mut TcpStream) -> Result<Response,std::io::Error> {
    let mut length_buffer = [0u8;4];
    stream.read_exact(&mut length_buffer)?;

    let mut response_buffer = vec![0u8; u32::from_be_bytes(length_buffer) as usize];
    stream.read_exact(&mut response_buffer)?;
    let response: Response = serde_json::from_slice(&response_buffer).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    Ok(response)
}

pub fn send_response(response: Response, stream:&mut TcpStream) -> Result<(), std::io::Error> {
    let ser_response = serde_json::to_vec(&response).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    stream.write_all(&(ser_response.len() as u32).to_be_bytes())?;
    stream.write_all(&ser_response)?;
    stream.flush()?;
    Ok(())
}
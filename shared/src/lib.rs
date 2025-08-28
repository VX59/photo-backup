use std::net::TcpStream;
use std::io::Read;
use std::io::Write;
use bincode::{Decode, Encode};
use serde::Deserialize;
use serde::Serialize;
use anyhow::Result;

#[derive(Debug, Encode, Decode)]
pub struct FileHeader {
    pub file_name: String,
    pub file_size: u64,
    pub file_ext: String,
    pub file_datetime: std::time::SystemTime,
}

#[derive(Serialize, Deserialize, PartialEq)]
pub enum ResponseCodes {
    OK,
    NotFound,
    Empty,
    NotConnected,
    InternalError,
    Duplicate,
}

impl std::fmt::Display for ResponseCodes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResponseCodes::OK => write!(f, "OK"),
            ResponseCodes::NotFound => write!(f, "Not Found"),
            ResponseCodes::Empty => write!(f, "Empty"),
            ResponseCodes::NotConnected => write!(f,"Not Connected"),
            ResponseCodes::InternalError => write!(f, "Internal Server Error"),
            ResponseCodes::Duplicate => write!(f, "Duplicate"),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct Response {
    pub status_code: ResponseCodes,
    pub status_message: String,
    pub body: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
pub enum RequestTypes {
    CreateRepo,
    GetRepos,
    SetStoragePath,
    StartStream,
    DisconnectStream,
    RemoveRepository,
}

#[derive(Serialize, Deserialize)]
pub struct Request {
    pub request_type:RequestTypes,
    pub body: Vec<u8>,
}

pub fn read_response(stream:&mut TcpStream) -> Result<Response,std::io::Error> {
    let mut length_buffer = [0u8;4];
    stream.read_exact(&mut length_buffer)?;

    let mut response_buffer = vec![0u8; u32::from_be_bytes(length_buffer) as usize];
    stream.read_exact(&mut response_buffer)?;
    let response: Response = serde_json::from_slice(&response_buffer)?;
    Ok(response)
}

pub fn send_response(response: Response, stream:&mut TcpStream) -> Result<(), std::io::Error> {
    let ser_response = serde_json::to_vec(&response)?;
    stream.write_all(&(ser_response.len() as u32).to_be_bytes())?;
    stream.write_all(&ser_response)?;
    Ok(())
}

pub fn read_request(stream: &mut TcpStream) -> Result<Request, std::io::Error> {
    let mut length_buffer = [0u8; 4];
    stream.read_exact(&mut length_buffer)?;

    let mut request_buffer = vec![0u8; u32::from_be_bytes(length_buffer) as usize];
    stream.read_exact(&mut request_buffer)?;
    let request: Request = serde_json::from_slice(&request_buffer)?;
    Ok(request)
}

pub fn send_request(request:Request, stream:&mut TcpStream) -> Result<(), std::io::Error> {
    let ser_request = serde_json::to_vec(&request)?;
    stream.write_all(&(ser_request.len() as u32).to_be_bytes())?;
    stream.write_all(&ser_request)?;
    Ok(())
}
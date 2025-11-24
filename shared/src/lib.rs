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
    pub relative_path: String,
    pub file_size: u64,
    pub file_ext: String,
    pub file_datetime: std::time::SystemTime,
}
use std::collections::HashMap;

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
    GetRepoTree,
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


#[derive(Serialize,Deserialize, Default, Debug, Clone)]
pub struct Tree {
    pub version: i32,
    pub content: HashMap<String,Vec<String>>, // a list of every directory's contents
    pub history: HashMap<i32,String>, // a list of modifications
    pub path: String,
    pub name: String,
}

impl Tree {
    pub fn load_from_file(path: &str) -> Self {
        let tree_content = std::fs::read_to_string(path)
        .unwrap_or_else(|_| {
            println!("Tree file not found, using default tree.");
            String::new()
        });
        serde_json::from_str(&tree_content).unwrap_or_else(|_| {
            println!("Failed to parse tree file, using default tree.");
            Tree::default()
        })
    }

    pub fn save_to_file(&self, path: &str) {
        if let Ok(tree_content) = serde_json::to_string_pretty(self) {
            if let Err(e) = std::fs::write(path, tree_content) {
                eprintln!("Failed to write tree file: {}", e);

            }
        }   
    }
    
    pub fn apply_history(&mut self, history_index:i32) {
        let start_index = history_index-self.version;
        for i in start_index..=self.version{
            if let Some(path_str) = self.history.get(&(i as i32)) {
                let path: Vec<&str> =   path_str.split('/').skip(2).collect();
                for window in path.windows(2) {
                    let parent = window[0].to_string();
                    let child = window[1].to_string();

                    let entry = self.content.entry(parent)
                        .or_insert_with(Vec::new);
                    if !entry.contains(&child) {
                        entry.push(child);
                    }
                }
            }
        }
    }

    pub fn add_history(&mut self, new_entry:String) {
        self.history.insert(self.version, new_entry);
        self.version += 1;
    }
}

#[derive(Serialize,Deserialize, Default, Debug, Clone)]
pub struct Config {
    pub repo_list: Vec<String>,
    pub path: String,
}

impl Config {
    pub fn load_from_file(path: &str) -> Self {
        let config_content = std::fs::read_to_string(path)
        .unwrap_or_else(|_| {
            println!("Config file not found, using default configuration.");
            String::new()
        });
        serde_json::from_str(&config_content).unwrap_or_else(|_| {
            println!("Failed to parse config file, using default configuration.");
            Config::default()
        })
    }

    pub fn save_to_file(&self, path: &str) {
        if let Ok(config_content) = serde_json::to_string_pretty(self) {
            if let Err(e) = std::fs::write(path, config_content) {
                eprintln!("Failed to write config file: {}", e);

            }
        }   
    }
    pub fn remove_repo(&mut self, repo:String) {
        if self.repo_list.contains(&repo) {
            self.repo_list.retain(|r| r != &repo);
            self.save_to_file(&self.path);
        } else {
            eprintln!("Repo does not exist in config.");
        }
    }
    
    pub fn add_repo(&mut self, repo:String) {
        if !self.repo_list.contains(&repo) {
            self.repo_list.push(repo);
            self.save_to_file(&self.path);
        } else {
            eprintln!("Repo already exists in config.");
        }
    }
}
pub trait Log {
    fn log_response(&self, response:&Response)  -> anyhow::Result<()>;
}

pub trait Notify {
    fn notify_app(&self, response:&Response) -> anyhow::Result<()>;
}
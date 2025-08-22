use std::collections::HashMap;
use std::thread::JoinHandle;
use shared::send_request;
use shared::RequestTypes;
use shared::ResponseCodes;
use std::io;
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::mpsc;
use shared::{read_response, Request};
use crate::app::{Commands,Config};
use crate::filestreamclient::FileStreamClient;

pub struct ImageClient {
    pub app_tx: mpsc::Sender<Commands>,
    rx: mpsc::Receiver<Commands>,
    stop_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
    config: Config,
    command_stream: Option<TcpStream>,
    repo_threads: HashMap<String, (std::thread::JoinHandle<()>,std::sync::Arc<std::sync::atomic::AtomicBool>)>,
}

impl ImageClient {
    pub fn new(app_tx: mpsc::Sender<Commands>,rx:mpsc::Receiver<Commands>,stop_flag: std::sync::Arc<std::sync::atomic::AtomicBool>) -> Self {
        let config_path = PathBuf::from("photo-client-config.json");
        let config: Config = Config::load_from_file(config_path.to_str().unwrap());

        ImageClient {
            app_tx,
            rx, 
            stop_flag,
            config,
            command_stream: None,
            repo_threads: HashMap::new(),
        }
    }

    pub fn connect(&mut self) -> anyhow::Result<()> {
        match TcpStream::connect(self.config.server_address.as_str()) {
            Ok(s) => {
                self.command_stream = Some(s);

                if let Some(stream) = self.command_stream.as_mut() {
                    // Handle the connection response
                    
                    let connection_response = read_response(stream)?;
                    let connection_response_message = String::from_utf8_lossy(&connection_response.body);
                    let connection_response_code = connection_response.status_code;

                    self.app_tx.send(Commands::Log(format!("{} | [ {} ]",connection_response_code,  connection_response_message))).unwrap();

                    // Send storage directory path to server

                    let request = Request {
                        request_type: RequestTypes::SetStoragePath,
                        body: self.config.server_storage_directory.as_bytes().to_vec(),
                    };

                    send_request(request, stream)?;

                    // read the storage directory response

                    let response = read_response(stream)?;
                    let response_message = String::from_utf8_lossy(&response.body);
                    let response_code = response.status_code;
                    
                    self.app_tx.send(Commands::Log(format!("{} | [ {} ]", response_code, response_message))).unwrap();
                    self.get_repositories()?;
                }
                
                // listen to the app for commands
                while !self.stop_flag.load(std::sync::atomic::Ordering::Relaxed) {
                    match self.rx.try_recv() {
                        Ok(new_command) => {
                            match new_command {
                                Commands::CreateRepo(msg) => {
                                    self.create_repository(msg.to_string())?;
                                }
                                Commands::StartStream(repo) => {

                                    // request a file streaming channel - the server will open another port
                                    let stop_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                                    let file_streaming_client_handle = self.initiate_file_streaming_client(repo.to_string(), stop_flag.clone())?;

                                    self.repo_threads.insert(repo, (file_streaming_client_handle, stop_flag));
                                }
                                Commands::DisconnectStream(repo) => {
                                    println!("disconnecting from the repo");

                                    match self.repo_threads.remove(&repo) {
                                        Some((handle,stop_flag)) => {
                                            stop_flag.store(true, std::sync::atomic::Ordering::Relaxed);
                                            self.app_tx.send(Commands::Log(format!("{} client thread stop flag set", repo).to_string())).unwrap();

                                            if let Err(_e) = handle.join() {
                                                self.app_tx.send(Commands::Log(format!("{} client thread failed to join", repo).to_string())).unwrap();
                                            }
                                        },
                                        None => {
                                            self.app_tx.send(Commands::Log(format!("Not connected to {}", repo).to_string())).unwrap();
                                        }
                                    }

                                    if let Some(stream) = self.command_stream.as_mut() {
                                        let request = Request {
                                            request_type:RequestTypes::DisconnectStream,
                                            body: repo.as_bytes().to_vec(),
                                        };

                                        send_request(request, stream)?;

                                        let response = read_response(stream)?;
                                        let response_code = response.status_code;
                                        let response_message = String::from_utf8_lossy(&response.body);

                                        self.app_tx.send(Commands::Log(format!("{} | [ {} ]", response_code, response_message))).unwrap();
                                    }
                                }
                                _ => {},
                            }
                        }
                        Err(std::sync::mpsc::TryRecvError::Empty) => { /* just continue */ },
                        Err(e) => return Err(anyhow::Error::new(e)),
                    }
                }
                
                if let Some(stream) = self.command_stream.as_mut() {
                    stream.shutdown(std::net::Shutdown::Both).ok();
                }
                self.app_tx.send(Commands::Log("Photo Client stopped.".to_string())).ok();

            },
            Err(e) => {
                self.app_tx.send(Commands::Log("Photo Client stopped.".to_string())).ok();
                self.stop_flag.store(true, std::sync::atomic::Ordering::Relaxed);
                return Err(anyhow::Error::new(std::io::Error::new(std::io::ErrorKind::Other, e)));
            },
        }
        Ok(())
    }

    pub fn get_repositories(&mut self) -> io::Result<()> { 
        let request = Request {
            request_type: RequestTypes::GetRepos,
            body: vec![0u8,0],
        };

        if let Some(stream) = self.command_stream.as_mut() {
            send_request(request, stream)?;
            let response = read_response(stream)?;
            let response_message = String::from_utf8_lossy(&response.body);
            let response_code = response.status_code;

            if response_code == ResponseCodes::OK {
                // send the repo list to the app
                let available_repositories:Vec<String> = serde_json::from_slice(&response.body)?;
                self.app_tx.send(Commands::PostRepos(available_repositories)).unwrap();
            } else {
                self.app_tx.send(Commands::Log(format!("{} | [ {} ]", response_code, response_message))).unwrap();
            }

        } else {
            self.app_tx.send(Commands::Log("Failed to get repositories, client not connected.".to_string())).ok();
        }
        Ok(())
    }

    pub fn initiate_file_streaming_client(&mut self, repo:String, stop_flag: std::sync::Arc<std::sync::atomic::AtomicBool>) -> io::Result<JoinHandle<()>> {
        if let Some(stream) = self.command_stream.as_mut() {
            let request = Request {
                request_type: RequestTypes::StartStream,
                body: repo.as_bytes().to_vec(), // implement security stuff here
            };

            send_request(request, stream)?;

            let response = read_response(stream)?;
            let file_stream_address = String::from_utf8_lossy(&response.body).to_string();
            
            self.app_tx.send(Commands::Log(format!("{} | [ {} ]", response.status_code, response.status_message))).unwrap();

            let mut file_stream = TcpStream::connect(file_stream_address.as_str())?;

            // handshake to confirm connection
            
            let file_stream_response = read_response(&mut file_stream)?;
            let response_message = String::from_utf8_lossy(&file_stream_response.body);

            self.app_tx.send(Commands::Log(format!("{} | [ {} ]", file_stream_response.status_code, response_message))).unwrap();

            // run the file streaming channel on a seperate thread

            let config_clone = self.config.clone();
            let app_tx_clone = self.app_tx.clone();
            let stop_flag_clone = stop_flag.clone();

            return Ok(std::thread::spawn(move || {
                let mut file_stream_client = FileStreamClient::new(file_stream, config_clone, app_tx_clone, stop_flag_clone);
                if let Err(e) = file_stream_client.run() {
                    file_stream_client.app_tx.send(Commands::Log(format!("Error starting streaming channel {}",e))).unwrap();
                }
            }));

        }
        Err(std::io::Error::new(std::io::ErrorKind::NotConnected,"Main stream is not connected"))
    }

    pub fn create_repository(&mut self, repo_name:String) -> io::Result<()> {
        
        if let Some(stream) = self.command_stream.as_mut() {
            let request = Request {
                request_type: RequestTypes::CreateRepo,
                body: repo_name.as_bytes().to_vec(),
            };

            send_request(request, stream)?;

            let response = read_response(stream)?;
            let response_message = String::from_utf8_lossy(&response.body);
            let _ = self.app_tx.send(Commands::Log(format!("{} | [ {} ]", response.status_code, response_message))).unwrap();

            if response.status_code == ResponseCodes::OK {
                self.get_repositories()?;
            }
        }
        Ok(())
    }

}
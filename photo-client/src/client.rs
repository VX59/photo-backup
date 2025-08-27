use std::collections::HashMap;
use std::thread::JoinHandle;
use shared::send_request;
use shared::RequestTypes;
use shared::Response;
use shared::ResponseCodes;
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::mpsc;
use shared::{read_response, Request};
use crate::app::ConnectionStatus;
use crate::app::RepoConfig;
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

                // Handle the connection response
                if let Some(stream) = &mut self.command_stream {
                    let response = read_response(stream)?;
                    self.log_response(&response)?;
                    
                    self.app_tx.send(Commands::UpdateConnectionStatus(ConnectionStatus::Connected))?;
                }
                
                // Send storage directory path to server
                if let Some(stream) = &mut self.command_stream {

                    let request = Request {
                        request_type: RequestTypes::SetStoragePath,
                        body: self.config.server_storage_directory.as_bytes().to_vec(),
                    };

                    send_request(request, stream)?;
                    let response = read_response(stream)?;
                    self.log_response(&response)?;
                
                    self.get_repositories()?;
                }

                // listen to the app for commands
                self.app_request_handler()?;

                // kill all repo connections
                let repo_list = self.repo_threads.keys().cloned().collect::<Vec<_>>();
                for repo in repo_list {
                    self.disconnect_repository(&repo)?;
                }

                // kill the client
                if let Some(stream) = self.command_stream.as_mut() {
                    stream.shutdown(std::net::Shutdown::Both)?;
                }
                self.app_tx.send(Commands::UpdateConnectionStatus(ConnectionStatus::Disconnected))?;
                self.app_tx.send(Commands::Log("Photo Client stopped.".to_string()))?;
            },
            Err(e) => {
                self.stop_flag.store(true, std::sync::atomic::Ordering::Relaxed);
                self.app_tx.send(Commands::UpdateConnectionStatus(ConnectionStatus::Disconnected))?;
                self.app_tx.send(Commands::Log("Photo Client stopped.".to_string()))?;
                return Err(anyhow::anyhow!(e));
            },
        }
        Ok(())
    }
    fn app_request_handler(&mut self) -> anyhow::Result<()> {
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
                            self.disconnect_repository(&repo)?;                               
                        }
                        _ => {},
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => { /* just continue */ },
                Err(e) => return Err(anyhow::Error::new(e)),
            }
        }
        Ok(())
    }

    pub fn disconnect_repository(&mut self, repo:&String) -> anyhow::Result<()>{
        match self.repo_threads.remove(repo) {
            Some((handle,stop_flag)) => {
                stop_flag.store(true, std::sync::atomic::Ordering::Relaxed);
                self.app_tx.send(Commands::Log(format!("{} client thread stop flag set", repo).to_string()))?;

                if let Err(_e) = handle.join() {
                    self.app_tx.send(Commands::Log(format!("{} client thread failed to join", repo).to_string()))?;
                    return Err(anyhow::anyhow!(format!("{} client thread failed to join", repo)));
                }
            },
            None => {
                self.app_tx.send(Commands::Log(format!("Not connected to {}", repo).to_string()))?;
            }
        }

        if let Some(stream) = self.command_stream.as_mut() {
            let request = Request {
                request_type:RequestTypes::DisconnectStream,
                body: repo.as_bytes().to_vec(),
            };

            send_request(request, stream)?;

            let response = read_response(stream)?;
            self.log_response(&response)?;

            let new_status = match response.status_code {
                ResponseCodes::OK => ConnectionStatus::Disconnected,
                _ => ConnectionStatus::Connected,
            };

            self.app_tx.send(Commands::UpdateRepoStatus((repo.clone(),new_status)))?;
            
        }
        Ok(())
    }

    pub fn get_repositories(&mut self) -> anyhow::Result<()> { 
        let request = Request {
            request_type: RequestTypes::GetRepos,
            body: vec![0u8,0],
        };

        if let Some(stream) = self.command_stream.as_mut() {
            send_request(request, stream)?;
            let response = read_response(stream)?;
            self.log_response(&response)?;

            if response.status_code == ResponseCodes::OK {
                // send the repo list to the app
                let available_repositories:Vec<String> = serde_json::from_slice(&response.body)?;
                self.app_tx.send(Commands::PostRepos(available_repositories))?;

                for (repo_name, config) in self.config.repo_config.clone() {
                    if config.auto_connect & self.config.repo_config.contains_key(&repo_name){
                        
                        // request a file streaming channel - the server will open another port
                        let stop_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                        let file_streaming_client_handle = self.initiate_file_streaming_client(repo_name.to_string(), stop_flag.clone())?;

                        self.repo_threads.insert(repo_name, (file_streaming_client_handle, stop_flag));
                    }
                }
            }

        } else {
            self.app_tx.send(Commands::Log("Failed to get repositories, client not connected.".to_string()))?;
        }
        Ok(())
    }

    pub fn initiate_file_streaming_client(&mut self, repo:String, stop_flag: std::sync::Arc<std::sync::atomic::AtomicBool>) -> anyhow::Result<JoinHandle<()>> {
        if let Some(stream) = self.command_stream.as_mut() {
            let request = Request {
                request_type: RequestTypes::StartStream,
                body: repo.as_bytes().to_vec(), // implement security stuff here
            };

            send_request(request, stream)?;

            let response = read_response(stream)?;
            self.log_response(&response)?;

            let mut file_stream = TcpStream::connect(String::from_utf8_lossy(&response.body).to_string().as_str())?;

            // handshake to confirm connection .. blocking
            let response = read_response(&mut file_stream)?;
            self.log_response(&response)?;

            let new_status = match response.status_code {
                ResponseCodes::OK => ConnectionStatus::Connected,
                _ => ConnectionStatus::Disconnected,
            };

            self.app_tx.send(Commands::UpdateRepoStatus((repo.clone(),new_status)))?;

            // run the file streaming channel on a seperate thread
            
            let config = Config::load_from_file("photo-client-config.json");
            let app_tx_clone = self.app_tx.clone();
            let stop_flag_clone = stop_flag.clone();

            return Ok(std::thread::spawn(move || {
                let mut file_stream_client = FileStreamClient::new(repo,file_stream, config, app_tx_clone, stop_flag_clone);
                if let Err(e) = file_stream_client.run() {
                    let _ = file_stream_client.app_tx.send(Commands::Log(format!("Error starting streaming channel {}",e)));
                }
            }));

        }
        Err(anyhow::anyhow!("Main stream is not connected"))
    }

    pub fn create_repository(&mut self, repo_name:String) -> anyhow::Result<()> {
        
        if let Some(stream) = self.command_stream.as_mut() {
            let request = Request {
                request_type: RequestTypes::CreateRepo,
                body: repo_name.as_bytes().to_vec(),
            };

            send_request(request, stream)?;

            let response = read_response(stream)?;
            self.log_response(&response)?;

            if response.status_code == ResponseCodes::OK {
                self.get_repositories()?;
            }
        }

        let repo_config = RepoConfig {
            recursive_backup: false,
            auto_connect: false,
            watch_directory: "".to_string(),
        };

        self.config.repo_config.insert(repo_name, repo_config);
        self.config.save_to_file("photo-client-config.json");

        Ok(())
        
    }
}

pub trait Log {
    fn log_response(&self, response:&Response)  -> anyhow::Result<()>;
}

impl Log for ImageClient {
    fn log_response(&self, response:&Response) -> anyhow::Result<()> {   
        let response_message = String::from_utf8_lossy(&response.body);
        let _ = self.app_tx.send(Commands::Log(format!("{} | [ {} ]", response.status_code, response_message)))?;
        Ok(())
    }
}
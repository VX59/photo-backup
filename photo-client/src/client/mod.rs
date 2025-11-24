use std::{sync::mpsc, collections::HashMap, thread::JoinHandle, net::TcpStream,
        path::PathBuf, sync::Arc, sync::atomic};
use shared::{send_request,RequestTypes,Response,ResponseCodes,Tree,
            read_response, Request, Log, Notify};
use crate::app::{Commands, ClientConfig, ConnectionStatus};
use crate::filestreamclient::FileStreamClient;

mod repository_managment;

pub struct Client {
    pub app_tx: mpsc::Sender<Commands>,
    rx: mpsc::Receiver<Commands>,
    stop_flag: Arc<atomic::AtomicBool>,
    config: ClientConfig,
    command_stream: Option<TcpStream>,
    repo_threads: HashMap<String, (std::thread::JoinHandle<()>,Arc<atomic::AtomicBool>)>,
    trees: HashMap<String,Tree>
}

impl Client {
    pub fn new(app_tx: mpsc::Sender<Commands>,rx:mpsc::Receiver<Commands>,stop_flag:Arc<atomic::AtomicBool>) -> Self {
        let config_path = PathBuf::from("photo-client-config.json");

        Client {
            app_tx,
            rx, 
            stop_flag,
            config: ClientConfig::load_from_file(config_path.to_str().unwrap()),
            command_stream: None,
            repo_threads: HashMap::new(),
            trees: HashMap::new()
        }
    }

    pub fn connect(&mut self) -> anyhow::Result<()> {
        match TcpStream::connect(self.config.server_address.as_str()) {
            Ok(s) => {
                if std::path::Path::new("photo-client/trees").exists() == false {
                    std::fs::create_dir_all("trees")?;
                }
                self.command_stream = Some(s);

                // Handle the connection response
                if let Some(stream) = &mut self.command_stream {
                    let response = read_response(stream)?;
                    self.log_response(&response)?;
                    self.notify_app(&response)?;
                    self.app_tx.send(Commands::UpdateConnectionStatus(ConnectionStatus::Connected))?;
                }

                if !self.config.server_storage_directory.is_empty() {
                    self.get_repositories()?
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
                self.app_tx.send(Commands::Notify("Client stopped.".to_string()))?;
            },
            Err(e) => {
                self.stop_flag.store(true, std::sync::atomic::Ordering::Relaxed);
                self.app_tx.send(Commands::UpdateConnectionStatus(ConnectionStatus::Disconnected))?;
                self.app_tx.send(Commands::Notify("Client stopped.".to_string()))?;
                return Err(anyhow::anyhow!(e));
            },
        }
        Ok(())
    }
    fn app_request_handler(&mut self) -> anyhow::Result<()> {
        while !self.stop_flag.load(atomic::Ordering::Relaxed) {
            match self.rx.try_recv() {
                Ok(new_command) => {
                    match new_command {
                        Commands::CreateRepo(msg) => self.create_repository(msg.to_string())?,
                        Commands::GetRepoTree(repo_name) => self.get_repo_tree(repo_name)?,
                        Commands::SetStoragePath(storage_directory) => self.set_storage_path(storage_directory)?,
                        Commands::StartStream(repo_name, watch_directory) => {
                            let stop_flag = std::sync::Arc::new(atomic::AtomicBool::new(false));
                            let file_streaming_client_handle = self.start_stream(repo_name.to_string(), watch_directory, stop_flag.clone())?;
                            self.repo_threads.insert(repo_name, (file_streaming_client_handle, stop_flag));
                        }
                        Commands::DisconnectStream(repo) => self.disconnect_repository(&repo)?,
                        Commands::RemoveRepository(repo) => {
                            self.disconnect_repository(&repo)?;
                            self.remove_repository(&repo)?;
                            self.get_repositories()?;
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

    fn set_storage_path(&mut self, storage_directory:String) ->anyhow::Result<()> {
        if let Some(stream) = &mut self.command_stream {

            let request = Request {
                request_type: RequestTypes::SetStoragePath,
                body: storage_directory.as_bytes().to_vec(),
            };

            send_request(request, stream)?;
            let response = read_response(stream)?;
            self.log_response(&response)?;
            self.notify_app(&response)?;

            if response.status_code == ResponseCodes::OK {
                self.config.save_to_file("./photo-client-config.json");

            }
            self.get_repositories()?;
        }
        Ok(())
    }
    
    fn start_stream(&mut self, repo:String, watch_directory:String, stop_flag: Arc<atomic::AtomicBool>) -> anyhow::Result<JoinHandle<()>> {
        if let Some(stream) = self.command_stream.as_mut() {
            let request = Request {
                request_type: RequestTypes::StartStream,
                body: repo.as_bytes().to_vec(), // implement security stuff here
            };

            send_request(request, stream)?;

            let response = read_response(stream)?;
            self.log_response(&response)?;
            self.notify_app(&response)?;

            let file_streaming_service = String::from_utf8_lossy(&response.body).to_string();
            let mut file_stream = TcpStream::connect(file_streaming_service)?;

            // handshake to confirm connection .. blocking
            let response = read_response(&mut file_stream)?;
            self.log_response(&response)?;
            self.notify_app(&response)?;

            let new_status = match response.status_code {
                ResponseCodes::OK => ConnectionStatus::Connected,
                _ => ConnectionStatus::Disconnected,
            };

            self.app_tx.send(Commands::UpdateRepoStatus((repo.clone(),new_status)))?;

            // run the file streaming channel on a seperate thread
            let app_tx_clone = self.app_tx.clone();
            return Ok(std::thread::spawn(move || {
                let mut file_stream_client = FileStreamClient::new(file_stream, watch_directory, app_tx_clone, stop_flag);
                if let Err(e) = file_stream_client.run() {
                    let _ = file_stream_client.app_tx.send(Commands::Log(format!("Error starting streaming channel {}",e)));
                }
            }));
        }
        Err(anyhow::anyhow!("Main stream is not connected"))
    }

}

impl Notify for Client {
    fn notify_app(&self, response:&Response) -> anyhow::Result<()> {   
        let response_message = String::from_utf8_lossy(&response.body);
        let _ = self.app_tx.send(Commands::Notify(format!("{}", response_message)))?;
        Ok(())
    }
}

impl Log for Client {
    fn log_response(&self, response:&Response) -> anyhow::Result<()> {   
        let response_message = String::from_utf8_lossy(&response.body);
        let _ = self.app_tx.send(Commands::Log(format!("{} | [ {} ]", response.status_code, response_message)))?;
        Ok(())
    }
}
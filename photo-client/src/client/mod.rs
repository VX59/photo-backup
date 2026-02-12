use std::{sync::mpsc, collections::HashMap, thread::JoinHandle, net::TcpStream, sync::Arc, sync::atomic};
use shared::{BatchJob, Log, Notify, Request, RequestTypes, Response, ResponseCodes, Tree, read_response, send_request};
use crate::app::{Commands, ClientConfig, ConnectionStatus};
use crate::filestreamclient::{BatchLoader, BatchLoaderCallback, RepoEventListener};

mod client_repository_managment;

pub struct Client {
    pub app_tx: mpsc::Sender<Commands>,
    app_rx: mpsc::Receiver<Commands>,
    stop_flag: Arc<atomic::AtomicBool>,
    config: ClientConfig,
    command_stream: Option<TcpStream>,
    repo_threads: HashMap<String, (std::thread::JoinHandle<()>,Arc<atomic::AtomicBool>)>,
    batch_loader_job_tx: Option<mpsc::Sender<BatchJob>>,
    batch_loader_callback_rx: Option<mpsc::Receiver<BatchLoaderCallback>>,
    batch_loader_join_handle: Option<JoinHandle<anyhow::Result<()>>>,
    trees: HashMap<String,Tree>
}

impl Client {
    pub fn new(app_tx: mpsc::Sender<Commands>,app_rx:mpsc::Receiver<Commands>,stop_flag:Arc<atomic::AtomicBool>, config:ClientConfig) -> Self {

        Client {
            app_tx, app_rx,
            stop_flag,
            config: config,
            command_stream: None,
            repo_threads: HashMap::new(),
            trees: HashMap::new(),
            batch_loader_job_tx: None,
            batch_loader_callback_rx: None,
            batch_loader_join_handle: None,
        }
    }

    pub fn connect(&mut self) -> anyhow::Result<()> { // returns a join handle for the batch loader
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

                // dispatch batch loader
                if let Some(stream) = &mut self.command_stream {
                    let request = Request {
                        request_type: RequestTypes::StartBatchProcessor,
                        body: vec![],
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
                    
                    let app_tx_clone = self.app_tx.clone();
                    let stop_flag_clone = self.stop_flag.clone();
                    let (callback_tx, rx) = mpsc::channel::<BatchLoaderCallback>();
                    self.batch_loader_callback_rx = Some(rx);
                    let mut batch_loader = BatchLoader::new(file_stream, stop_flag_clone, app_tx_clone, callback_tx);

                    (self.batch_loader_job_tx, self.batch_loader_join_handle) = match batch_loader.listen() {
                        Ok((tx, join_handle)) => (Some(tx),Some(join_handle)),
                        Err(_) => (None,None)
                    };
                }

                if !self.config.server_storage_directory.is_empty() {
                    self.get_repositories()?
                }

                // listen to the app for commands
                self.app_request_handler()?;
                
                // kill all repo connections
                let repo_list = self.repo_threads.keys().cloned().collect::<Vec<_>>();
                for repo_name in repo_list {
                    self.disconnect_repository(&repo_name)?;
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
            match self.app_rx.try_recv() {
                Ok(new_command) => {
                    match new_command {
                        Commands::DiscoverUntracked(repo_name) => self.discover_untracked(repo_name)?,
                        Commands::CreateRepo(msg) => self.create_repository(msg.to_string())?,
                        Commands::GetRepoTree(repo_name) => self.get_repo_tree(repo_name)?,
                        Commands::SetStoragePath(storage_directory) => self.set_storage_path(storage_directory)?,
                        Commands::StartEventListener(repo_name, watch_directory) => {
                            let stop_flag = std::sync::Arc::new(atomic::AtomicBool::new(false));
                            let file_streaming_client_handle = self.start_event_listener(repo_name.to_string(), watch_directory, stop_flag.clone())?;
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
                self.config.server_storage_directory = storage_directory;
                self.config.save_to_file("photo-client-config.json");

            }
            self.get_repositories()?;
        }
        Ok(())
    }
    
    fn start_event_listener(&mut self, repo_name:String, watch_directory:String, stop_flag: Arc<atomic::AtomicBool>) -> anyhow::Result<JoinHandle<()>> {
        if let (Some(batch_loader_tx), Some(repo_config)) = (self.batch_loader_job_tx.clone(), self.config.repo_config.get(&repo_name)) {
            let track_modifications = repo_config.track_modifications.clone();
            let repo_name_clone = repo_name.clone();
            let join_handle = std::thread::spawn(move || {
                let mut file_stream_client = RepoEventListener::new(repo_name_clone, watch_directory, batch_loader_tx, stop_flag, track_modifications);
                if let Err(e) = file_stream_client.run() {
                    eprintln!("event listener failed to run {e:?}")
                }
            });
            self.app_tx.send(Commands::UpdateRepoStatus((repo_name,ConnectionStatus::Connected)))?;
            return Ok(join_handle)
        }
        Err(anyhow::anyhow!("Batch loader tx not found"))
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
use std::thread::JoinHandle;
use shared::send_request;
use shared::RequestTypes;
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
    stream: Option<TcpStream>,
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
            stream: None,
        }
    }

    pub fn connect(&mut self) -> io::Result<()> {
        match TcpStream::connect(self.config.server_address.as_str()) {
            Ok(s) => {
                self.stream = Some(s);

                if let Some(stream) = self.stream.as_mut() {
                    // Handle the connection response
                    
                    let connection_response = read_response(stream)?;
                    let connection_response_message = String::from_utf8_lossy(&connection_response.body);
                    let connection_response_code = connection_response.status_code;

                    let _ = self.app_tx.send(Commands::Log(format!("{} | [ {} ]",connection_response_code,  connection_response_message))).unwrap();

                    // Send storage directory path to server

                    let request = Request {
                        request_type: RequestTypes::SetStoragePath,
                        body: self.config.server_storage_directory.as_bytes().to_vec(),
                    };

                    send_request(request, stream)?;

                    // Handle the storage directory response

                    let response = read_response(stream)?;
                    let response_message = String::from_utf8_lossy(&response.body);
                    let response_code = response.status_code;
                    
                    if response_code == 200 {
                        // send the repo list to the app
                        let available_repositories:Vec<String> = serde_json::from_slice(&response.body)?;
                        self.app_tx.send(Commands::PostRepos(available_repositories)).unwrap();
                    } else {
                        self.app_tx.send(Commands::Log(format!("{} | [ {} ]", response_code, response_message))).unwrap();
                    }
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
                                    let mut file_stream = self.initiate_file_streaming_client(repo.to_string())?;
                                    
                                }
                                _ => {},
                            }
                        }
                        Err(std::sync::mpsc::TryRecvError::Empty) => { /* just continue */ },
                        Err(e) => return Err(std::io::Error::new(std::io::ErrorKind::Other, e)),
                    }
                }
                
                if let Some(stream) = self.stream.as_mut() {
                    stream.shutdown(std::net::Shutdown::Both).ok();
                }
                self.stream.take();
                self.app_tx.send(Commands::Log("Photo Client stopped.".to_string())).ok();

            },
            Err(e) => self.app_tx.send(Commands::Log(format!("error {}",e).to_string())).unwrap(),
        }
        Ok(())
    }

    pub fn initiate_file_streaming_client(&mut self, repo:String) -> io::Result<JoinHandle<()>> {
        if let Some(stream) = self.stream.as_mut() {
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
            let stop_flag_clone = self.stop_flag.clone();


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
        
        if let Some(stream) = self.stream.as_mut() {
            let request = Request {
                request_type: RequestTypes::CreateRepo,
                body: repo_name.as_bytes().to_vec(),
            };

            send_request(request, stream)?;

            let response = read_response(stream)?;
            let response_message = String::from_utf8_lossy(&response.body);
            let _ = self.app_tx.send(Commands::Log(format!("{} | [ {} ]", response.status_code, response_message))).unwrap();
        }
        Ok(())
    }

}
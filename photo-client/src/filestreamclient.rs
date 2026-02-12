use std::{io::prelude::*, net::TcpStream, path::{Path,PathBuf}, sync::{Arc, atomic, mpsc}, thread::JoinHandle, time::Duration};
use bincode::{config, encode_into_slice};
use notify::{Watcher,RecommendedWatcher, RecursiveMode, EventKind};
use shared::{read_response, FileHeader, Job, BatchJob};
use std::{fs, thread::sleep};

use crate::app::{Commands};

pub struct RepoEventListener {
    repo_name: String,
    watch_directory:String,
    batch_job_tx:mpsc::Sender<BatchJob>,
    stop_flag:Arc<atomic::AtomicBool>,
    track_modifications:bool,
}


pub struct BatchLoader {
    stream:Option<TcpStream>, // communicates with the server
    stop_flag: Option<Arc<atomic::AtomicBool>>,
    rx: Option<mpsc::Receiver<BatchJob>>,
    pub tx: mpsc::Sender<BatchJob>,
    pub app_tx:Option<mpsc::Sender<Commands>>,
}

impl BatchLoader {
    pub fn new(stream:TcpStream, stop_flag: Arc<atomic::AtomicBool>, app_tx:mpsc::Sender<Commands>) -> Self {
        let (tx, rx) = mpsc::channel::<BatchJob>();
        BatchLoader { 
            stream: Some(stream),
            stop_flag: Some(stop_flag),
            rx: Some(rx)
            ,tx,
            app_tx: Some(app_tx),
        }
    }

    pub fn listen(&mut self) -> anyhow::Result<(mpsc::Sender<BatchJob>, JoinHandle<anyhow::Result<()>>)>{
        
        if let Some(stream) = &mut self.stream {
            stream.flush()?;
        }

        let rx = self.rx.take().expect("batch job receiver not set");
        let stop_flag = self.stop_flag.take().expect("stop flag not given");
        let mut stream = self.stream.take().expect("stream not given");
        let app_tx = self.app_tx.take().expect("app tx not given");
        let join_handle:JoinHandle<anyhow::Result<()>> = std::thread::spawn(move || {
            while !stop_flag.load(atomic::Ordering::Relaxed) {
                match rx.try_recv() {
                    Ok(batch_job) => {
                        let chunk_size = 1024 * 1024 * 1;

                        let batch_size = batch_job.jobs.len() as u32;
                        stream.write_all(&batch_size.to_be_bytes())?;

                        for job in batch_job.jobs {
                            let mut header_bytes = vec![0u8;1024];
                            let header_size = encode_into_slice(&job.file_header, &mut header_bytes, config::standard())? as u32;
                            header_bytes.truncate(header_size as usize);
                            stream.write_all(&header_size.to_be_bytes())?;
                            stream.write_all(&header_bytes)?;
                            let mut remaining = &job.data[..];

                            while !remaining.is_empty() {
                                let take = std::cmp::min(remaining.len(), chunk_size);
                                let chunk = &remaining[..take];
                                stream.write_all(&(take as u32).to_be_bytes())?;
                                stream.write_all(&chunk)?;
                                remaining = &remaining[take..];
                            }
                            stream.write_all(&0u32.to_be_bytes())?;
                        }

                        let response = read_response(&mut stream)?;
                        let response_message: std::borrow::Cow<'_, str> = String::from_utf8_lossy(&response.body);
                        app_tx.send(Commands::Notify(format!("{}", response_message)))?;
                        app_tx.send(Commands::Log(format!("{} | [ {} ]", response.status_code, response_message)))?;

                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {},
                    Err(e) => return Err(anyhow::anyhow!(e))
                }
            }

            stream.shutdown(std::net::Shutdown::Both).ok();
            let message = "Streaming client stopped.".to_string();
            app_tx.send(Commands::Log(message.clone()))?;
            app_tx.send(Commands::Notify(message.clone()))?;

            Ok(())
        });


        Ok((self.tx.clone(), join_handle))
    }
}

impl RepoEventListener {
    pub fn new(
            repo_name: String,
            watch_directory:String,
            batch_job_tx:mpsc::Sender<BatchJob>,
            stop_flag: Arc<atomic::AtomicBool>,
            track_modifications:bool) -> Self {
        RepoEventListener {
            repo_name,
            watch_directory,
            batch_job_tx,
            stop_flag,
            track_modifications,
        }
    }

    pub fn run(&mut self) -> anyhow::Result<()> {
        let (wtx, wrx) = mpsc::channel();
        let mut watcher = match RecommendedWatcher::new(move |res| 
            match wtx.send(res) {
                Ok(res) => res,
                Err(_) => {},

            }, notify::Config::default())
            {
        Ok(w) => w,
        Err(e) => {
            eprintln!("Failed to create file watcher: {}", e);
            return Err(anyhow::anyhow!("Failed to create file watcher"));
            }
        };

        if let Err(e) = watcher.watch(Path::new(&self.watch_directory), RecursiveMode::Recursive) {
            return Err(anyhow::anyhow!("Failed to watch directory: {} {}",self.watch_directory, e));
        }

        while !self.stop_flag.load(atomic::Ordering::Relaxed) {
            match wrx.try_recv() {
                Ok(event) => {
                    let new_event = match event {
                        Ok(ev) => ev,
                        Err(e) => {
                            eprintln!("Watch error: {:?}", e);
                            continue;
                        }
                    };
                    if let EventKind::Create(notify::event::CreateKind::File) = new_event.kind {
                        for path in new_event.paths.clone() {

                            let file_ext = path.extension()
                                .and_then(|s| s.to_str())
                                .unwrap_or("unknown");
                            if file_ext == "part" {
                                continue
                            }
                            
                            loop {
                                match is_file_stable(&path, 100, 10) {
                                    Ok(true) => break, // File is stable, exit loop
                                    Ok(false) => {
                                        // File not stable yet, wait and try again
                                        std::thread::sleep(Duration::from_millis(1000));
                                        continue;
                                    }
                                    Err(e) => {
                                        eprintln!("Error checking file: {:?}", e);
                                        // Decide whether to continue or break
                                        break; // or return an error
                                    }
                                }
                            }
                            if let Err(e) = self.submit_job(path, self.repo_name.clone()) {
                                eprintln!("Failed to send image: {}", e);
                                break;
                            };
                        }
                    }
                },
                Err(mpsc::TryRecvError::Empty) => {
                    std::thread::sleep(Duration::from_secs(1));
                    continue;
                },
                Err(mpsc::TryRecvError::Disconnected) => {
                    break;
                }
            }
        }
        Ok(())
    }

    fn prepare_job(&self, local_path:PathBuf, repo_name: String) -> anyhow::Result<BatchJob> {
        if !local_path.exists() {
            return Err(anyhow::anyhow!("File not found"));
        }

        let mut last_size = 0;
        loop {
            if self.stop_flag.load(atomic::Ordering::Relaxed) {
                return Err(anyhow::anyhow!("Job cancelled"));
            }
            let metadata = std::fs::metadata(&local_path)?;
            let current_size = metadata.len();
            if current_size == last_size {
                break;
            }
            last_size = current_size;
        }

        let file_name = local_path.file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| anyhow::anyhow!("Invalid file name"))?;

        let file_ext = local_path.extension()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown");

        let file_datetime = local_path.metadata()?.created()?;

        println!("local path {}\n", local_path.clone().to_string_lossy());

        let file_bytes: Vec<u8> = std::fs::read(local_path.clone())?;

        let file_header = FileHeader {
            repo_name: repo_name,
            file_name: file_name.to_string(), 
            file_size: file_bytes.len() as usize,
            file_ext: file_ext.to_string(),
            file_datetime: file_datetime,
        };

        println!("File size: {} bytes", file_bytes.len());

        // a singleton job
        let job = Job {
            file_header: file_header,
            data: file_bytes,
        };

        Ok(BatchJob::new(vec![job]))
    }

    fn submit_job(&mut self, local_path:PathBuf, repo_name:String) -> anyhow::Result<()> {
        let batch_job = self.prepare_job(local_path, repo_name)?;
        self.batch_job_tx.send(batch_job)?;
        Ok(())
    }
}

fn is_file_stable(path: &Path, check_interval_ms: u64, stability_checks: u32) -> anyhow::Result<bool> {
    let mut previous_size = match fs::metadata(path).map(|m| m.len()) {
        Ok(size) => size,
        Err(_) => return Ok(false),
    };
    
    for _ in 0..stability_checks {
        sleep(Duration::from_millis(check_interval_ms));
        let current_size = match fs::metadata(path).map(|m| m.len()) {
            Ok(size) => size,
            Err(_) => return Ok(false),
        };
        
        if current_size != previous_size {
            return Ok(false);
        }
        previous_size = current_size;
    }
    Ok(true)
}
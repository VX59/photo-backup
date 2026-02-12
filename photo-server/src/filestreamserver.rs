use std::{
    collections::HashMap,io::prelude::*, net::{TcpListener, TcpStream}, path::PathBuf, sync::{Arc, atomic}, thread::JoinHandle
};
use shared::{send_response, Response, Tree, FileHeader, Job};

pub fn initiate_batch_processor(storage_directory: PathBuf, listener:TcpListener, stop_flag:Arc<atomic::AtomicBool>) -> anyhow::Result<JoinHandle<()>>{   
    match listener.accept() {
        Ok((mut file_stream, _)) => {
            
            let response: Response = Response {
                status_code:shared::ResponseCodes::OK,
                status_message:"OK".to_string(),
                body: "Created batch processor".as_bytes().to_vec(),
            };

            send_response(response, &mut file_stream)?;

            return Ok(std::thread::spawn(move || {
                println!("file stream thread initiated");
                
                let mut file_stream_server = BatchProcessor::new(storage_directory, file_stream, stop_flag);
                match file_stream_server.listen() {
                    Ok(_) => {} // handle result
                    Err(e) => println!("{}",e)
                };
            }));
        },
        Err(e) => return Err(anyhow::anyhow!(e)),
    }
}

struct BatchProcessor {
    storage_directory: PathBuf,
    stream:TcpStream,
    stop_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
    trees:HashMap<String, Tree>,
}

impl BatchProcessor {
    pub fn new(storage_directory: PathBuf, stream:TcpStream, stop_flag:std::sync::Arc<std::sync::atomic::AtomicBool>) -> Self{
        BatchProcessor {
            storage_directory,
            stream,
            stop_flag,
            trees: HashMap::new(),
        }
    }

    pub fn listen(&mut self) -> anyhow::Result<()> {


        while !self.stop_flag.load(std::sync::atomic::Ordering::Relaxed) {
            match self.process_batch_job() {
                Ok(_) => {
                    let response = Response {
                        status_code:shared::ResponseCodes::OK,
                        status_message: "OK".to_string(),
                        body: format!("processed batch job").as_bytes().to_vec(),
                    };
                    
                    if let Err(e) = send_response(response, &mut self.stream) {
                        println!("{}", e);
                        break;
                    }
                }
                Err(e) => {
                    if e.to_string().contains("UnexpectedEnd") || 
                    e.to_string().contains("EOF") {
                        println!("Connection closed by client");
                    } else {
                        println!("Connection error: {}", e);
                    }
                    break;
                }
            }
        }
        Ok(())
    }

    fn process_batch_job(&mut self) -> anyhow::Result<()> {
        let mut batch_header_length_buffer = [0u8; 4];
        
        self.stream.read_exact(&mut batch_header_length_buffer)?;

        let batch_num_jobs: u32 = u32::from_be_bytes(batch_header_length_buffer);
        let mut jobs = Vec::<Job>::new();

        for i in 0..batch_num_jobs {
            let mut header_size_buf = [0u8; 4];
            self.stream.read_exact(&mut header_size_buf)?;
            let header_size = u32::from_be_bytes(header_size_buf) as usize;

            let mut header_bytes = vec![0u8; header_size];
            self.stream.read_exact(&mut header_bytes)?;
            let (file_header, _):(FileHeader, usize) = bincode::decode_from_slice(&header_bytes, bincode::config::standard())?;

            let mut job_data_bytes = vec![0u8,file_header.file_size as u8];

                
            loop {
                let mut chunk_size_buf = [0u8; 4];
                self.stream.read_exact(&mut chunk_size_buf)?;
                let chunk_size = u32::from_be_bytes(chunk_size_buf) as usize;
                if chunk_size == 0 { break; }

                let mut chunk = vec![0u8; chunk_size];
                self.stream.read_exact(&mut chunk)?;
                job_data_bytes.extend_from_slice(&chunk);
            }

            jobs.push(Job {file_header: file_header, data: job_data_bytes});
        }

        for job in jobs {
            let file_header = job.file_header;
            let file_path = self.storage_directory.join(&file_header.repo_name).join(file_header.file_name);

            if !self.trees.contains_key(&file_header.repo_name) {
                let tree_path = PathBuf::from("trees").join(format!("{}.tree", &file_header.repo_name));
                let tree = Tree::load_from_file(tree_path.to_string_lossy().as_ref());
                self.trees.insert(file_header.repo_name.clone(), tree);
            }
            
            if let Some(tree) = self.trees.get_mut(&file_header.repo_name) {
                let start_index = tree.add_history( format!("+{}", file_path.to_str().unwrap().to_string()));
                tree.apply_history(start_index);
                tree.save_to_file(&tree.path);
            }

            println!("Receiving file: {} ({} bytes)", file_path.to_str().unwrap().to_string(), file_header.file_size);

            if let Some(parent) = file_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&file_path, &job.data)?;

        } 
        Ok(())
    }
}
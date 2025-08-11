use bincode::{Decode, Encode};

#[derive(Debug, Encode, Decode)]
pub struct FileHeader {
    pub file_name: String,
    pub file_size: u64,
    pub file_ext: String,
}
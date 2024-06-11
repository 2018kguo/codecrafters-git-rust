use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use sha1::{Digest, Sha1};
use hex;

#[allow(unused_imports)]
use std::env;
#[allow(unused_imports)]
use std::fs;
use std::io::prelude::*;

pub trait GitObject {
    // Method to serialize the object. This must be implemented by any struct implementing the trait.
    fn serialize(&self) -> Vec<u8>;

    // Method to deserialize data into the object. This must be implemented by any struct implementing the trait.
    fn deserialize(&mut self, data: &[u8]);

    fn fmt(&self) -> &[u8];
}

pub struct GitBlob {
    pub blob_data: Vec<u8>,
}

impl GitObject for GitBlob {
    fn fmt(&self) -> &[u8] {
        b"blob"
    }

    fn serialize(&self) -> Vec<u8> {
        self.blob_data.clone()
    }

    fn deserialize(&mut self, data: &[u8]) {
        self.blob_data = data.to_vec();
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    match args[1].as_str() {
        "init" => {
            fs::create_dir(".git").unwrap();
            fs::create_dir(".git/objects").unwrap();
            fs::create_dir(".git/refs").unwrap();
            fs::write(".git/HEAD", "ref: refs/heads/main\n").unwrap();
            println!("Initialized git directory")
        }
        "cat-file" => {
            let hash = &args[args.len() - 1];
            let object = read_object(hash);
            std::io::stdout().write_all(&object.serialize()).unwrap();
            std::io::stdout().flush().unwrap();
        }
        "hash-object" => {
            let file_path = &args[args.len() - 1];
            let data = fs::read(file_path).unwrap();
            let object = GitBlob {
                blob_data: data,
            };
            let hash = write_object(object);
            println!("{}", hash);
        }
        _ => {
            println!("unknown command: {}", args[1])
        }
    }
}

fn read_object(hash: &str) -> impl GitObject {
    let path = format!(".git/objects/{}/{}", &hash[..2], &hash[2..]);
    let data = fs::read(path).unwrap();
    let mut decoder = ZlibDecoder::new(data.as_slice());
    let mut decoded_bytes = Vec::new();
    decoder.read_to_end(&mut decoded_bytes).unwrap();
    let bytes_split_at_null = decoded_bytes.split(|&x| x == 0).collect::<Vec<&[u8]>>();
    let object_type = bytes_split_at_null[0];
    let _object_type_string = String::from_utf8(object_type.to_vec()).unwrap();

    let byte_contents = bytes_split_at_null[1];
    GitBlob {
        blob_data: byte_contents.to_vec(),
    }
}

fn write_object(object: impl GitObject) -> String {
    // returns the sha1 hash of the object
    let serialized = object.serialize();
    let mut result = Vec::new();
    result.extend_from_slice(object.fmt());
    result.push(b' ');
    result.extend_from_slice(serialized.len().to_string().as_bytes());
    result.push(b'\0');
    result.extend_from_slice(&serialized);
    let mut hasher = Sha1::new();
    hasher.update(&result);
    let hash_result = hasher.finalize();
    let sha_string = hex::encode(hash_result);
    let path = format!(".git/objects/{}/{}", &sha_string[..2], &sha_string[2..]);
    fs::create_dir_all(format!(".git/objects/{}", &sha_string[..2])).unwrap();
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&result).unwrap();
    let compressed = encoder.finish().unwrap();
    fs::write(path, compressed).unwrap();
    sha_string
}

use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use sha1::{Digest, Sha1};

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

enum GitObjectType {
    Blob(GitBlob),
    Tree(GitTree),
}

#[derive(Clone)]
pub struct GitTreeLeaf {
    pub mode: Vec<u8>,
    pub path: String,
    // big endian hex representation of the sha1 hash
    pub sha_hash: String,
}

fn tree_parse_one(raw_bytes: &[u8], start_index: usize) -> (GitTreeLeaf, usize) {
    let mut index = start_index;
    let mut mode = [0; 6];
    while raw_bytes[index] != b' ' {
        // mode is up to 6 bytes and is an octal representation of the file mode
        // stored in ascii.
        mode[index - start_index] = raw_bytes[index];
        index += 1;
    }
    if mode.len() == 5 {
        // normalize the mode to 6 bytes
        mode = [b'0', mode[0], mode[1], mode[2], mode[3], mode[4]];
    }
    let mut path = String::new();
    // there's a whitespace character between the mode and the path that we need to skip
    index += 1;
    // find the null byte that signals the end of the path
    while raw_bytes[index] != b'\0' {
        path.push(raw_bytes[index] as char);
        index += 1;
    }
    index += 1;
    let mut sha_hash = String::new();
    // the sha1 hash is 20 bytes long and in big endian format
    for _ in 0..20 {
        sha_hash.push_str(&format!("{:02x}", raw_bytes[index]));
        index += 1;
    }
    (
        GitTreeLeaf {
            mode: mode.to_vec(),
            path,
            sha_hash,
        },
        index,
    )
}

fn tree_parse(raw_bytes: &[u8]) -> Vec<GitTreeLeaf> {
    let mut index = 0;
    let mut result = Vec::new();
    while index < raw_bytes.len() {
        let (leaf, new_index) = tree_parse_one(raw_bytes, index);
        result.push(leaf);
        index = new_index;
    }
    result
}

fn tree_leaf_sort_key(leaf: &GitTreeLeaf) -> String {
    if leaf.mode.starts_with(b"10") {
        leaf.path.clone()
    } else {
        // directories are sorted with a trailing slash
        format!("{}\\", leaf.path)
    }
}

pub struct GitTree {
    pub leaves: Vec<GitTreeLeaf>,
}

impl GitObject for GitTree {
    fn fmt(&self) -> &[u8] {
        b"tree"
    }

    fn serialize(&self) -> Vec<u8> {
        // sort leaves by tree_leaf_sort_key
        // this is necessary because sorting paths matters for git

        let sorted_leaves = {
            let mut leaves = self.leaves.clone();
            leaves.sort_by_key(tree_leaf_sort_key);
            leaves
        };

        let mut result = Vec::new();
        for leaf in &sorted_leaves {
            result.extend_from_slice(&leaf.mode);
            result.push(b' ');
            result.extend_from_slice(leaf.path.as_bytes());
            result.push(b'\0');
            let hash_bytes = hex::decode(&leaf.sha_hash).unwrap();
            result.extend_from_slice(&hash_bytes);
        }
        result
    }

    fn deserialize(&mut self, data: &[u8]) {
        self.leaves = tree_parse(data);
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
            match object {
                GitObjectType::Blob(blob) => {
                    std::io::stdout().write_all(&blob.serialize()).unwrap();
                    std::io::stdout().flush().unwrap();
                }
                _ => {
                    println!("unexpected object type for cat-file"); 
                }
            }
        }
        "hash-object" => {
            let file_path = &args[args.len() - 1];
            let data = fs::read(file_path).unwrap();
            let object = GitBlob { blob_data: data };
            let hash = write_object(object);
            println!("{}", hash);
        }
        "ls-tree" => {
            let hash = &args[args.len() - 1];
            let object = read_object(hash);
            match object {
                GitObjectType::Tree(tree) => ls_tree(tree),
                _ => println!("not a tree object"),
            }
        }
        _ => {
            println!("unknown command: {}", args[1])
        }
    }
}

fn read_object(hash: &str) -> GitObjectType {
    let path = format!(".git/objects/{}/{}", &hash[..2], &hash[2..]);
    let data = fs::read(path).unwrap();
    let mut decoder = ZlibDecoder::new(data.as_slice());
    let mut decoded_bytes = Vec::new();
    decoder.read_to_end(&mut decoded_bytes).unwrap();
    let index_of_first_whitespace = decoded_bytes.iter().position(|&x| x == b' ').unwrap();
    let index_of_first_null = decoded_bytes.iter().position(|&x| x == 0).unwrap();
    let object_type = &decoded_bytes[..index_of_first_whitespace];
    let byte_contents = &decoded_bytes[index_of_first_null + 1..];

    match object_type {
        b"blob" => {
            let mut blob = GitBlob { blob_data: Vec::new() };
            blob.deserialize(byte_contents);
            GitObjectType::Blob(blob)
        },
        b"tree" => {
            let mut tree = GitTree { leaves: Vec::new() };
            tree.deserialize(byte_contents);
            GitObjectType::Tree(tree)
        },
        _ => panic!("unknown object type"),
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

fn ls_tree(tree: GitTree) {
    for leaf in tree.leaves {
        println!("{}", leaf.path);
    }
}

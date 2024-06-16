use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use sha1::{Digest, Sha1};

use chrono::{DateTime, Local};
#[allow(unused_imports)]
use std::env;
#[allow(unused_imports)]
use std::fs;
use std::io::prelude::*;
use std::time;

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
            // trim the leading null byte from the mode if it's there
            let mode = if leaf.mode.starts_with(b"0") {
                &leaf.mode[1..]
            } else {
                &leaf.mode
            };
            result.extend_from_slice(mode);
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

pub struct GitCommit {
    pub commit_data: String
} 

impl GitObject for GitCommit {
    fn fmt(&self) -> &[u8] {
        b"commit"
    }

    fn serialize(&self) -> Vec<u8> {
        self.commit_data.clone().as_bytes().to_vec()
    }

    fn deserialize(&mut self, data: &[u8]) {
        self.commit_data = String::from_utf8(data.to_vec()).unwrap();
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
            let contents = object.serialize();
            let object_type = object.fmt();
            let hash = write_object(contents.as_slice(), object_type);
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
        "write-tree" => {
            let tree_hash = write_tree(".");
            println!("{}", tree_hash);
        }
        "commit-tree" => {
            let parent_hash_index = args.iter().position(|x| x == "-p").unwrap();
            let parent_hash = &args[parent_hash_index + 1];
            let message_index = args.iter().position(|x| x == "-m").unwrap();
            let message = &args[message_index + 1];
            let commit_tree_index = args.iter().position(|x| x == "commit-tree").unwrap();
            let tree_hash = &args[commit_tree_index + 1];
            let commit_hash = commit(tree_hash, message, parent_hash);
            println!("{}", commit_hash);
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
            let mut blob = GitBlob {
                blob_data: Vec::new(),
            };
            blob.deserialize(byte_contents);
            GitObjectType::Blob(blob)
        }
        b"tree" => {
            let mut tree = GitTree { leaves: Vec::new() };
            tree.deserialize(byte_contents);
            GitObjectType::Tree(tree)
        }
        _ => panic!("unknown object type"),
    }
}

fn write_object(contents: &[u8], object_type: &[u8]) -> String {
    // returns the sha1 hash of the object
    let serialized = contents;
    let mut result = Vec::new();
    result.extend_from_slice(object_type);
    result.push(b' ');
    result.extend_from_slice(serialized.len().to_string().as_bytes());
    result.push(b'\0');
    result.extend_from_slice(serialized);
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

fn write_tree(path: &str) -> String {
    // creates a tree object from the current working directory and saves tree files recursively
    // returns the sha1 hash of the tree object
    //
    // mode, path, sha1 hash
    let mut entries: Vec<(Vec<u8>, String, String)> = Vec::new();
    for entry in fs::read_dir(path).unwrap() {
        let entry = entry.unwrap();
        let metadata = entry.metadata().unwrap();
        let mode = if metadata.is_dir() {
            b"040000".to_vec()
        } else {
            b"100644".to_vec()
        };
        let file_name = entry.file_name().into_string().unwrap();
        let entry_path = entry.path();
        if file_name == ".git" {
            continue;
        }
        if metadata.is_dir() {
            let tree_sha_hash = write_tree(entry_path.to_str().unwrap());
            entries.push((mode, file_name, tree_sha_hash));
        } else {
            let blob_contents = fs::read(entry_path.clone()).unwrap();
            let blob = GitBlob {
                blob_data: blob_contents,
            };
            let blob_contents = blob.serialize();
            let sha_hash = write_object(blob_contents.as_slice(), blob.fmt());
            entries.push((mode, file_name, sha_hash));
        }
    }
    let tree = GitTree {
        leaves: entries
            .iter()
            .map(|(mode, entry_path, sha_hash)| GitTreeLeaf {
                mode: mode.clone(),
                path: entry_path.clone(),
                sha_hash: sha_hash.clone(),
            })
            .collect(),
    };
    let tree_ser = tree.serialize();
    write_object(tree_ser.as_slice(), tree.fmt())
}

fn commit(tree_hash: &str, message: &str, parent_hash: &str) -> String {
    // creates a commit object with the current tree and the given message
    // returns the sha1 hash of the commit object
    //
    // tree 22264ec0ce9da29d0c420e46627fa0cf057e709a
    // parent 03f882ade69ad898aba73664740641d909883cdc
    // author Ben Hoyt <benhoyt@gmail.com> 1493170892 -0500
    // committer Ben Hoyt <benhoyt@gmail.com> 1493170892 -0500
    //
    // Fix cat-file size/type/pretty handling\n
    //
    let hardcoded_author_name = "Kevin Guo";
    let hardcoded_author_email = "kev.guo123@gmail.com";

    // get the current epoch time in seconds
    let timestamp = time::SystemTime::now()
        .duration_since(time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let now: DateTime<Local> = Local::now();

    // Get the UTC offset in hours and minutes
    let offset = now.offset();
    let offset_hours = offset.local_minus_utc() / 3600;
    let offset_minutes = (offset.local_minus_utc() % 3600) / 60;

    // Format the offset as +HHMM or -HHMM
    let offset = format!("{:+03}{:02}", offset_hours, offset_minutes);
    // get the offset from UTC, formatted as -0500 or +0000
    let author_contents = format!(
        "{} <{}> {} {}",
        hardcoded_author_name, hardcoded_author_email, timestamp, offset
    );

    let author_line = format!("author {}", author_contents);
    let parent_line = format!("parent {}", parent_hash);
    let committer_line = format!("committer {}", author_contents);

    let tree_line = format!("tree {}", tree_hash);

    let commit_lines = vec![
        tree_line,
        parent_line,
        author_line,
        committer_line,
        "".to_string(),
        message.to_string(),
        "".to_string(),
    ];
    let commit_contents = commit_lines.join("\n");

    let commit = GitCommit {
        commit_data: commit_contents,
    };
    let commit_contents = commit.serialize();
    write_object(commit_contents.as_slice(), commit.fmt())
}

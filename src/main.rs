use flate2::read::ZlibDecoder;
#[allow(unused_imports)]
use std::env;
#[allow(unused_imports)]
use std::fs;
use std::io::prelude::*;

enum ObjectResult {
    Blob(String)
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
                ObjectResult::Blob(contents_ascii) => {
                    print!("{}", contents_ascii);
                    std::io::stdout().flush().unwrap();
                }
            }
        }
        _ => {
            println!("unknown command: {}", args[1])
        }
    }
}

fn read_object(hash: &str) -> ObjectResult {
    let path = format!(".git/objects/{}/{}", &hash[..2], &hash[2..]);
    let data = fs::read(path).unwrap();
    let mut decoder = ZlibDecoder::new(data.as_slice());
    let mut decoded_bytes = Vec::new();
    decoder.read_to_end(&mut decoded_bytes).unwrap();
    let bytes_split_at_null = decoded_bytes.split(|&x| x == 0).collect::<Vec<&[u8]>>();
    let object_type = bytes_split_at_null[0];
    let _object_type_string = String::from_utf8(object_type.to_vec()).unwrap();

    let byte_contents = bytes_split_at_null[1];
    let contents_ascii = String::from_utf8(byte_contents.to_vec()).unwrap();
    ObjectResult::Blob(contents_ascii)
}

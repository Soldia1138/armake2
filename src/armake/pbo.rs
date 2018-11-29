use std::str;
use std::io::{Read, Seek, Write, SeekFrom, Error, Cursor, BufReader, BufWriter};
use std::fs::{File, create_dir_all, read_dir};
use std::collections::{HashMap};
use std::path::{PathBuf};

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use openssl::hash::{Hasher, MessageDigest};
use time::*;

use armake::config::*;

struct PBOHeader {
    filename: String,
    packing_method: u32,
    original_size: u32,
    reserved: u32,
    timestamp: u32,
    data_size: u32
}

pub struct PBO {
    files: HashMap<String, Cursor<Box<[u8]>>>,
    header_extensions: HashMap<String, String>,
    headers: Vec<PBOHeader>
}

impl PBOHeader {
    fn read<I: Read>(input: &mut I) -> Result<PBOHeader, Error> {
        Ok(PBOHeader {
            filename: read_cstring(input),
            packing_method: input.read_u32::<LittleEndian>()?,
            original_size: input.read_u32::<LittleEndian>()?,
            reserved: input.read_u32::<LittleEndian>()?,
            timestamp: input.read_u32::<LittleEndian>()?,
            data_size: input.read_u32::<LittleEndian>()?
        })
    }

    fn write<O: Write>(&self, output: &mut O) -> Result<(), Error> {
        output.write_all(self.filename.as_bytes())?;
        output.write_all(b"\0")?;
        output.write_u32::<LittleEndian>(self.packing_method)?;
        output.write_u32::<LittleEndian>(self.original_size)?;
        output.write_u32::<LittleEndian>(self.reserved)?;
        output.write_u32::<LittleEndian>(self.timestamp)?;
        output.write_u32::<LittleEndian>(self.data_size)?;
        Ok(())
    }
}

impl PBO {
    fn read<I: Read>(input: &mut I) -> Result<PBO, Error> {
        let mut headers: Vec<PBOHeader> = Vec::new();
        let mut first = true;
        let mut header_extensions: HashMap<String, String> = HashMap::new();

        loop {
            let header = PBOHeader::read(input)?;
            // todo: garbage filter

            if header.packing_method == 0x56657273 {
                if !first { unreachable!(); }

                loop {
                    let s = read_cstring(input);
                    if s.len() == 0 { break; }

                    header_extensions.insert(s, read_cstring(input));
                }
            } else if header.filename == "" {
                break;
            } else {
                headers.push(header);
            }

            first = false;
        }

        let mut files: HashMap<String, Cursor<Box<[u8]>>> = HashMap::new();
        for header in &headers {
            let mut buffer: Box<[u8]> = vec![0; header.data_size as usize].into_boxed_slice();
            input.read_exact(&mut buffer)?;
            files.insert(header.filename.clone(), Cursor::new(buffer));
        }

        Ok(PBO {
            files: files,
            header_extensions: header_extensions,
            headers: headers
        })
    }

    fn from_directory(directory: PathBuf) -> Result<PBO, Error> {
        let file_list = list_files(&directory)?;
        let mut files: HashMap<String, Cursor<Box<[u8]>>> = HashMap::new();
        let mut header_extensions: HashMap<String,String> = HashMap::new();

        for path in file_list {
            let relative = path.strip_prefix(&directory).unwrap();
            let name: String = relative.to_str().unwrap().replace("/", "\\");

            let mut file = File::open(&path)?;

            if name == "$PBOPREFIX$" {
                // @todo: do this properly
                let mut content = String::new();
                file.read_to_string(&mut content);
                for l in content.split("\n") {
                    if l.len() == 0 { break; }

                    let eq: Vec<String> = l.split("=").map(|s| s.to_string()).collect();
                    if eq.len() == 1 {
                        header_extensions.insert("prefix".to_string(), l.to_string());
                    } else {
                        header_extensions.insert(eq[0].clone(), eq[1].clone());
                    }
                }

                continue;
            }

            let mut buffer: Vec<u8> = Vec::new();
            file.read_to_end(&mut buffer)?;

            files.insert(name, Cursor::new(buffer.into_boxed_slice()));
        }

        if header_extensions.get("prefix").is_none() {
            let prefix: String = directory.file_name().unwrap().to_str().unwrap().to_string();
            header_extensions.insert("prefix".to_string(), prefix);
        }

        Ok(PBO {
            files: files,
            header_extensions: header_extensions,
            headers: Vec::new()
        })
    }

    fn write<O: Write>(&self, output: &mut O) -> Result<(), Error> {
        let mut headers: Cursor<Vec<u8>> = Cursor::new(Vec::new());

        let ext_header = PBOHeader {
            filename: "".to_string(),
            packing_method: 0x56657273,
            original_size: 0,
            reserved: 0,
            timestamp: 0,
            data_size: 0
        };
        ext_header.write(&mut headers);

        if let Some(prefix) = self.header_extensions.get("prefix") {
            headers.write_all(b"prefix\0")?;
            headers.write_all(prefix.as_bytes())?;
            headers.write_all(b"\0")?;
        }

        for (key, value) in self.header_extensions.iter() {
            if key == "prefix" { continue; }

            headers.write_all(key.as_bytes())?;
            headers.write_all(b"\0")?;
            headers.write_all(value.as_bytes())?;
            headers.write_all(b"\0")?;
        }
        headers.write_all(b"\0")?;

        let mut files_sorted: Vec<(String,&Cursor<Box<[u8]>>)> = self.files.iter().map(|(a,b)| (a.clone(),b)).collect();
        files_sorted.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));

        for (name, cursor) in &files_sorted {
            let header = PBOHeader {
                filename: name.clone(),
                packing_method: 0,
                original_size: cursor.get_ref().len() as u32,
                reserved: 0,
                timestamp: 0,
                data_size: cursor.get_ref().len() as u32
            };

            header.write(&mut headers)?;
        }

        let header = PBOHeader {
            packing_method: 0,
            ..ext_header
        };
        header.write(&mut headers)?;

        let mut h = Hasher::new(MessageDigest::sha1()).unwrap();

        output.write_all(headers.get_ref());
        h.update(headers.get_ref()).unwrap();

        for (_, cursor) in &files_sorted {
            output.write_all(cursor.get_ref())?;
            h.update(cursor.get_ref()).unwrap();
        }

        output.write_all(&[0]);
        output.write_all(&*h.finish().unwrap())?;

        Ok(())
    }
}

fn list_files(directory: &PathBuf) -> Result<Vec<PathBuf>, Error> {
    let mut files: Vec<PathBuf> = Vec::new();

    for entry in read_dir(directory)? {
        let path = entry?.path();
        if path.is_dir() {
            for f in list_files(&path)? {
                files.push(f);
            }
        } else {
            files.push(path);
        }
    }

    Ok(files)
}

pub fn cmd_inspect<I: Read>(input: &mut I) -> i32 {
    let pbo = PBO::read(input).expect("Failed to read PBO.");

    if pbo.header_extensions.len() > 0 {
        println!("Header extensions:");
        for (key, value) in pbo.header_extensions.iter() {
            println!("- {}={}", key, value);
        }
        println!("");
    }

    println!("# Files: {}\n", pbo.files.len());

    println!("Path                                                  Method  Original    Packed");
    println!("                                                                  Size      Size");
    println!("================================================================================");
    for header in pbo.headers {
        println!("{:50} {:9} {:9} {:9}", header.filename, header.packing_method, header.original_size, header.data_size);
    }

    0
}

pub fn cmd_cat<I: Read, O: Write>(input: &mut I, output: &mut O, name: String) -> i32 {
    let pbo = PBO::read(input).expect("Failed to read PBO.");

    match pbo.files.get(&name) {
        Some(cursor) => {
            output.write_all(cursor.get_ref()).expect("Failed to write output.");
            0
        },
        None => {
            eprintln!("not found");
            1
        }
    }
}

pub fn cmd_unpack<I: Read>(input: &mut I, output: PathBuf) -> i32 {
    let pbo = PBO::read(input).expect("Failed to read PBO.");

    create_dir_all(&output).expect("Failed to create output folder");

    if pbo.header_extensions.len() > 0 {
        let prefix_path = output.join(PathBuf::from("$PBOPREFIX$"));
        let mut prefix_file = File::create(prefix_path).expect("Failed to open prefix file.");

        for (key, value) in pbo.header_extensions.iter() {
            prefix_file.write_all(format!("{}={}\n", key, value).as_bytes());
        }
    }

    for (file_name, cursor) in pbo.files.iter() {
        // @todo: windows
        let path = output.join(PathBuf::from(file_name.replace("\\", "/")));
        let mut file = File::create(path).expect("Failed to open output file.");
        file.write_all(cursor.get_ref());
    }

    0
}

pub fn cmd_pack<O: Write>(input: PathBuf, output: &mut O) -> i32 {
    let pbo = PBO::from_directory(input).expect("Failed to read directory");

    pbo.write(output).expect("Failed to write PBO");

    0
}

pub fn cmd_build<O: Write>(input: PathBuf, output: &mut O) -> i32 {
    0
}

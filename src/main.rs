use std::env;
use std::io::Write;
use std::mem;
use std::fs::{self, File};
use std::io::{Seek, SeekFrom, Read, self};
use std::path::{Path, PathBuf};
use std::convert::TryInto;
use flate2::read::ZlibDecoder;
use rayon::prelude::*;

#[derive(Default)]
struct PyinstEntry {
    size: u32,
    offset: u32,
    compressed_size: u32,
    uncompressed_size: u32,
    compression_flag: u8,
    type_: u8,
    name: String
}


#[derive(Default)]
struct PyinstHeader {
    signature: [u8; 8],
    package_size: u32,
    toc_offset: u32,
    toc_size: u32,
    python_version: u32
}

const MAGIC_BASE: [u8; 8] = [
    b'M', b'E', b'I', 0x0C,
    0x0B, 0x0A, 0x0B, 0x0E
];

fn write_nested_file(base_path: &Path, file_path: &str, content: &[u8], compressed: bool) -> io::Result<()> {

    let full_path = base_path.join(file_path.replace("\\", "/"));

    if full_path.exists() {
        return Ok(());
    }

    if let Some(parent) = full_path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let mut file = fs::File::create(&full_path)?;
    if compressed {
        let mut decoder = ZlibDecoder::new(content);
        io::copy(&mut decoder, &mut file)?;
    } else {
        file.write_all(content)?;
    }


    Ok(())
}

fn parse_entry(fp: &mut File, overlay_offset: usize) -> PyinstEntry {

    let mut buffer = [0u8; 18];
    fp.read_exact(&mut buffer).expect("Read error");

    let size = u32::from_be_bytes(buffer[0..4].try_into().unwrap());

    // TotalSize - ((Size) Size + (Offset) Size + (CompressedSize) Size + (UncompressedSize) Size + (CompressionFlag) Size + (type) Size)
    let name_size = size - (mem::size_of::<u32>() as u32 * 4 + 1 + 1);

    let mut entry: PyinstEntry = Default::default();
    entry.size = size;
    entry.offset = overlay_offset as u32 + u32::from_be_bytes(buffer[4..8].try_into().unwrap());
    entry.compressed_size = u32::from_be_bytes(buffer[8..12].try_into().unwrap());
    entry.uncompressed_size = u32::from_be_bytes(buffer[12..16].try_into().unwrap());
    entry.compression_flag = buffer[16];
    entry.type_ = buffer[17];
    
    let mut buffer: Vec<u8> = vec![0u8; name_size as usize];

    fp.read_exact(&mut buffer).expect("Read error");
    if let Some(pos) = buffer.iter().position(|&b| b == 0) {
        buffer.truncate(pos);
    }
    entry.name = String::from_utf8(buffer).expect("Name error");

    if !entry.name.contains('.') {
        entry.name.push_str(".pyc");
    }

    entry
}

fn parse_header(fp: &mut File, header_offset: usize) -> PyinstHeader {


    let mut header: PyinstHeader = Default::default();

    fp.seek(SeekFrom::Start(header_offset as u64)).expect("Cannot seek to header");
    
    fp.read_exact(&mut header.signature).expect("Read error");
    
    let mut buffer = [0u8; 16];
    
    fp.read_exact(&mut buffer).expect("Read error");

    header.package_size = u32::from_be_bytes(buffer[0..4].try_into().unwrap());
    header.toc_offset = u32::from_be_bytes(buffer[4..8].try_into().unwrap());
    header.toc_size = u32::from_be_bytes(buffer[8..12].try_into().unwrap());
    header.python_version = u32::from_be_bytes(buffer[12..16].try_into().unwrap());

    fp.rewind().expect("Rewind error");

    header
}

fn find_header(fp: &mut File) -> Option<usize> {

    let mut content: Vec<u8> = Vec::new();
    fp.read_to_end(&mut content).expect("Cannot read file");

    let signature: &[u8] = &MAGIC_BASE;

    content.windows(8).position(|window| window == signature)
    
}

fn main() -> io::Result<()> {
    let args: Vec<String> = env::args().collect();
    
    if args.len() < 2 {
        println!("Usage: {} [file_path]", args[0]);
        std::process::exit(1);
    }

    let file_path = &args[1];

    println!("Extracting {}", file_path);

    let base_path = PathBuf::from(format!("{}_extracted", file_path));

    let mut fp = File::open(file_path)?;

    let mut file_content: Vec<u8> = Vec::new();
    fp.read_to_end(&mut file_content).expect("Cannot read file");
    let filesize = file_content.len();
    fp.rewind().expect("Rewind error");
    

    let offset = find_header(&mut fp).expect("Invalid pyinstaller file");
    println!("Got header offset at: {:#2X}", offset);


    // To count useless tail Bytes 
    fp.seek(SeekFrom::Start((offset + mem::size_of::<PyinstHeader>()) as u64)).expect("Seek error"); 
    
    let mut buffer: Vec<u8> = Vec::new();

    fp.read_to_end(&mut buffer).expect("Cannot read file");
    let tail_size = buffer.len();

    fp.rewind().expect("Rewind error");

    let header = parse_header(&mut fp, offset);
    
    let overlay_offset = filesize - header.package_size as usize - tail_size;

    println!("Package Size: {}\nToc Size: {}\nPython Version: {}.{}", header.package_size, header.toc_size, (header.python_version/100) as i32, (header.python_version%100) as i32);

    
    fp.seek(SeekFrom::Start(overlay_offset as u64 + header.toc_offset as u64 + tail_size as u64)).expect("Seek error"); 

    let mut bytes_read = 0;

    let mut toc: Vec<PyinstEntry> = Vec::new();

    let mut entry: PyinstEntry;

    while bytes_read < header.toc_size {
        entry = parse_entry(&mut fp, overlay_offset + tail_size);
        bytes_read += entry.size;
        toc.push(entry);
    }

    println!("Parsed {} entries", toc.len());


    toc.par_iter().for_each( |entry| {

        let content = &file_content[entry.offset as usize .. (entry.offset + entry.compressed_size) as usize];

        let compressed = entry.compression_flag == 1;

        write_nested_file(base_path.as_path(), entry.name.as_str(), content, compressed).expect("Write error");
    });

    println!("Extracted as: {}", base_path.to_str().expect("!"));

    Ok(())
}

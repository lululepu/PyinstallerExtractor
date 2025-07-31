use std::io::Write;
use std::mem;
use std::fs::{self, File};
use std::io::{Seek, SeekFrom, Read, self};
use std::path::{Path, PathBuf};
use std::convert::TryInto;
use flate2::read::ZlibDecoder;
use rayon::prelude::*;
use binrw::BinRead;
use clap::Parser;
use std::time::Instant;

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    #[arg(short, long)]
    input: String,

    #[arg(short, long, default_value = "")]
    output: String,
}


#[derive(Default, Debug)]
#[allow(dead_code)]
struct PyinstEntry {
    size: u32,
    offset: u32,
    compressed_size: u32,
    uncompressed_size: u32,
    compression_flag: u8,
    type_: u8,
    name: String
}

#[allow(dead_code)]
#[derive(BinRead)]
#[br(big)]
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

    // not using binrw cause idk how to parse null-terminated dynamic sized strings

    let mut buffer = [0u8; 18];
    fp.read_exact(&mut buffer).expect("Read error");

    let size = u32::from_be_bytes(buffer[0..4].try_into().unwrap());
    let offset = u32::from_be_bytes(buffer[4..8].try_into().unwrap()) + overlay_offset as u32;
    let compressed_size = u32::from_be_bytes(buffer[8..12].try_into().unwrap());
    let uncompressed_size = u32::from_be_bytes(buffer[12..16].try_into().unwrap());
    let compression_flag = buffer[16];
    let type_ = buffer[17];

    // name_size = TotalSize - ((Size) Size + (Offset) Size + (CompressedSize) Size + (UncompressedSize) Size + (CompressionFlag) Size + (type) Size)
    let name_size = size - (4 * 4 + 1 + 1);

    let mut buffer: Vec<u8> = vec![0u8; name_size as usize];

    fp.read_exact(&mut buffer).expect("Read error");
    if let Some(pos) = buffer.iter().position(|&b| b == 0) {
        buffer.truncate(pos);
    }
    let mut name = String::from_utf8(buffer).expect("Name error");
    if !name.contains('.') {
        name.push_str(".pyc");
    }

    PyinstEntry { 
        size: size,
        offset: offset,
        compressed_size: compressed_size,
        uncompressed_size: uncompressed_size,
        compression_flag: compression_flag,
        type_: type_,
        name: name
    }

}

fn parse_header(fp: &mut File, header_offset: usize) -> PyinstHeader {

    fp.seek(SeekFrom::Start(header_offset as u64)).expect("Cannot seek to header");
    let header = PyinstHeader::read(fp).expect("Error reading header");
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
    let start = Instant::now();

    let args = Args::parse();

    println!("Extracting {}", args.input);

    let mut base_path = PathBuf::new();

    if args.output.len() > 0 {
        base_path.push(args.output);
    } else {
        base_path.push(format!("{}_extracted", args.input));
    }

    let mut fp = File::open(args.input)?;

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

    println!("Package Size: {}\nToc Size: {}\nToc Offset: {:#2X}\nPython Version: {}.{}", header.package_size, header.toc_size, header.toc_offset,(header.python_version/100) as i32, (header.python_version%100) as i32);

    
    fp.seek(SeekFrom::Start(overlay_offset as u64 + header.toc_offset as u64 + 64)).expect("Seek error"); 

    let mut bytes_read = 0;

    let mut toc: Vec<PyinstEntry> = Vec::new();

    let mut entry: PyinstEntry;

    while bytes_read < header.toc_size {
        entry = parse_entry(&mut fp, overlay_offset + 64);
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

    let duration = start.elapsed();
    println!("Dump took: {} ms", duration.as_millis());

    Ok(())
}

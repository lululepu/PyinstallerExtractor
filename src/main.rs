use std::io::{Cursor, Write};
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
use std::io::BufWriter;
use mimalloc::MiMalloc;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

const ARCHIVE_ITEM_PYZ: u8             = b'z'; // zlib (pyz) - frozen Python code
const ARCHIVE_ITEM_PYSOURCE: u8        = b's'; // Python script (v3)
/*
const ARCHIVE_ITEM_BINARY: u8          = b'b'; // binary
const ARCHIVE_ITEM_DEPENDENCY: u8      = b'd'; // runtime option
const ARCHIVE_ITEM_ZIPFILE: u8         = b'Z'; // zlib (pyz) - frozen Python code
const ARCHIVE_ITEM_PYPACKAGE: u8       = b'M'; // Python package (__init__.py)
const ARCHIVE_ITEM_PYMODULE: u8        = b'm'; // Python module
const ARCHIVE_ITEM_DATA: u8            = b'x'; // data
const ARCHIVE_ITEM_RUNTIME_OPTION: u8  = b'o'; // runtime option
const ARCHIVE_ITEM_SPLASH: u8          = b'l'; // splash resources
const ARCHIVE_ITEM_SYMLINK: u8         = b'n'; // symbolic link
*/

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

#[allow(dead_code)]
#[derive(BinRead)]
#[br(little)]
struct PyzHeader {
    magic: [u8; 4],
    version: [u8; 4],
    toc_offset: u32
}



const PYINST_MAGIC_BASE: [u8; 8] = [
    b'M', b'E', b'I', 0x0C,
    0x0B, 0x0A, 0x0B, 0x0E
];

fn write_nested_file(base_path: &Path, entry: &PyinstEntry, file_content: &[u8], pyc_magic: [u8; 16]) -> io::Result<()> {

    let full_path = if entry.name.contains('\\') {
        base_path.join(entry.name.replace("\\", "/"))
    } else {
        base_path.join(&entry.name)
    };

    let content = &file_content[entry.offset as usize .. (entry.offset + entry.compressed_size) as usize];

    if full_path.exists() {
        return Ok(());
    }

    if let Some(parent) = full_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let file = fs::File::create(&full_path)?;
    let mut writer = BufWriter::new(file);

    if entry.compression_flag == 1 {
        let mut decoder = ZlibDecoder::new(content);

        // if valid magic add it
        if pyc_magic[0] != 0 && entry.type_ == ARCHIVE_ITEM_PYSOURCE {
            writer.write_all(&pyc_magic)?;
        }

        let mut buf = [0u8; 64 * 1024];
        loop {
            let len = decoder.read(&mut buf)?;
            if len == 0 {
                break;
            }
            writer.write_all(&buf[..len])?;
        }
    } else {
        writer.write_all(content)?;
    }

    writer.flush()?;
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

    if type_ == ARCHIVE_ITEM_PYSOURCE {
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
    

    let signature: &[u8] = &PYINST_MAGIC_BASE;
    
    let offset  = file_content.windows(8).position(|window| window == signature).expect("Invalid pyinstaller archive");

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

    let mut pyc_magic = [0u8; 16];

    while bytes_read < header.toc_size {
        entry = parse_entry(&mut fp, overlay_offset + 64);

        if entry.type_ == ARCHIVE_ITEM_PYZ {
            let mut content = Cursor::new(&file_content[entry.offset as usize .. (entry.offset + entry.compressed_size) as usize]);

            let pyz_header = PyzHeader::read(&mut content).expect("Invalid pyz header");

            pyc_magic[..4].copy_from_slice(&pyz_header.version);
        }

        bytes_read += entry.size;
        toc.push(entry);
    }

    println!("Parsed {} entries", toc.len());
    let duration = start.elapsed();
    println!("Parsing took: {} ms", duration.as_millis());
    if pyc_magic[0] == 0 {
        println!("Cannot find the python header...");
    }
    let start = Instant::now();

    toc.par_chunks(8).for_each(|chunk| {
        for entry in chunk {
            write_nested_file(base_path.as_path(), entry, &file_content, pyc_magic).expect("Write error");
        }
    });

    println!("Extracted as: {}", base_path.to_str().expect("!"));

    let duration = start.elapsed();
    println!("Extraction took: {} ms", duration.as_millis());

    Ok(())
}

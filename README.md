# PyInstaller Extractor in Rust

A Rust tool to extract embedded Python files from PyInstaller executables.

---

## About

This is my **first Rust project** learning Rust by building this extractor.

---

## Requirements

- Rust and Cargo  
- Crates: `flate2` and `rayon`

---

## Installation

Clone and build:

```bash
git clone https://github.com/your-username/pyinstaller-extractor-rust.git
cd pyinstaller-extractor-rust
cargo build --release
```

---

## Usage

```bash
extractor.exe [/path/to/file.exe]
```

Files are extracted to a folder named `{filename}_extracted`.

---

## Dependencies

In `Cargo.toml`:

```toml
[dependencies]
flate2 = "1.0"
rayon = "1.7"
```


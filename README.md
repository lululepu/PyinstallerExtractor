# PyInstaller Extractor in Rust

A Rust tool to extract embedded Python files from PyInstaller executables.

---

## About

This is my **first Rust project** learning Rust by building this extractor.

---

## Requirements

- Rust and Cargo  

---

## Installation

Clone and build:

```bash
git clone https://github.com/lululepu/PyinstallerExtractor.git
cd PyinstallerExtractor
cargo build -r
```

---

## Usage

```bash
extractor.exe -i [test.exe] -o [output]
```
---

## Dependencies

In `Cargo.toml`:

```toml
[dependencies]
binrw = "0.15.0"
clap = { version = "4.5.42", features = ["derive"] }
flate2 = "1.0"
rayon = "1.7"
mimalloc = "0.1.47"
```


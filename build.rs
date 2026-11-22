// Copyright 2026 Thobias Melfjord Knudsen
// SPDX-License-Identifier: Apache-2.0

//! Hands the seed its start graph, the file `LOGOS_START_GRAPH` names or none, and the key a graph
//! must carry to be read: a hash of everything the graph and the code that reads it are built
//! from (›The seed carries its start graph inside its binary‹ [[R386]]).

use std::path::{Path, PathBuf};

const INPUTS: [&str; 6] =
    ["src", "identities", "build.rs", "Cargo.toml", "Cargo.lock", "rust-toolchain.toml"];
const SETTINGS: [&str; 6] = [
    "TARGET",
    "PROFILE",
    "OPT_LEVEL",
    "DEBUG",
    "CARGO_ENCODED_RUSTFLAGS",
    "CARGO_CFG_TARGET_FEATURE",
];

fn main() {
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("cargo sets OUT_DIR"));
    println!("cargo:rerun-if-env-changed=LOGOS_START_GRAPH");
    let graph = match std::env::var_os("LOGOS_START_GRAPH").filter(|path| !path.is_empty()) {
        Some(path) => {
            println!("cargo:rerun-if-changed={}", Path::new(&path).display());
            std::fs::read(&path).unwrap_or_else(|e| {
                panic!("LOGOS_START_GRAPH names {}: {e}", Path::new(&path).display())
            })
        }
        None => Vec::new(),
    };
    std::fs::write(out.join("start_graph.bin"), graph).expect("OUT_DIR is writable");

    let mut key = Fnv::default();
    for input in INPUTS {
        println!("cargo:rerun-if-changed={input}");
        let mut files = Vec::new();
        walk(Path::new(input), &mut files);
        files.sort();
        for file in files {
            key.add(file.to_string_lossy().as_bytes());
            key.add(&std::fs::read(&file).expect("a source file is readable"));
        }
    }
    for setting in SETTINGS {
        key.add(std::env::var(setting).unwrap_or_default().as_bytes());
    }
    let rustc = std::env::var_os("RUSTC").expect("cargo sets RUSTC");
    let version = std::process::Command::new(rustc).arg("-vV").output().expect("rustc runs");
    key.add(&version.stdout);
    println!("cargo:rustc-env=LOGOS_BUILD_KEY={:016x}", key.0);
}

/// Every file at or under `path`.
fn walk(path: &Path, files: &mut Vec<PathBuf>) {
    if path.is_dir() {
        let entries = std::fs::read_dir(path).expect("a source folder is readable");
        for entry in entries {
            walk(&entry.expect("a source folder lists").path(), files);
        }
    } else if path.is_file() {
        files.push(path.to_path_buf());
    }
}

/// FNV-1a over each part and its length, so no two lists of parts hash alike by running together.
struct Fnv(u64);

impl Default for Fnv {
    fn default() -> Self {
        Fnv(0xcbf2_9ce4_8422_2325)
    }
}

impl Fnv {
    fn add(&mut self, part: &[u8]) {
        for &byte in (part.len() as u64).to_le_bytes().iter().chain(part) {
            self.0 = (self.0 ^ byte as u64).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
}

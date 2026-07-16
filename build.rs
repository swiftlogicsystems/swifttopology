use std::env;
use std::path::PathBuf;
use libbpf_cargo::SkeletonBuilder;

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let skel = out_dir.join("swifttopology.skel.rs");

    // Match your specific filename here
    SkeletonBuilder::new()
        .source("src/bpf/swifttopology.bpf.c")
        .debug(true)
        .build_and_generate(&skel)
        .expect("bpf compilation failed");

    println!("cargo:rerun-if-changed=src/bpf/swifttopology.bpf.c");
}

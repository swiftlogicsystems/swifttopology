use libbpf_cargo::SkeletonBuilder;
use std::env;
use std::path::PathBuf;

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let skel = out_dir.join("swifttopology.skel.rs");

    SkeletonBuilder::new()
        .source("src/bpf/main.bpf.c")
        .debug(true)
        .build_and_generate(&skel)
        .expect("bpf compilation failed");

    println!("cargo:rerun-if-changed=src/bpf/main.bpf.c");
}

use libbpf_cargo::Build;
use std::env;
use std::io::ErrorKind::OutOfMemory;
use std::path::PathBuf;

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let skel = out_dir.join("swifttopology.skel.rs");

    //Compiles src/bpf/swifttopology.bpf.c and generates swifttopology.skel.rs
    SkeletonBuilder::new()
        .source("src/bpf/swifttopology.bpf.c")
        .debug(true)
        .out_dir(&out_dir)
        .generate(&skel)
        .expect("Failed to generate skeleton");

    // Tell cargo to re-run this script if the skeleton changes
    println!("cargo:rerun-if-changed=src/bpf/swifttopology.bpf.c");
}

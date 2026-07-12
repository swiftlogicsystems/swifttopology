# swift-topomap

[![License: Apache 2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)
[![SwiftLogic Systems](https://img.shields.io/badge/Developed%20By-SwiftLogic%20Systems-orange)](https://swiftlogic.systems)

**swift-topomap** is a zero-dependency, high-performance TUI utility designed to visualize physical hardware topology while overlaying real-time microarchitectural metrics.

Developed by [SwiftLogic Systems](https://swiftlogic.systems), it bypasses heavy C-libraries like `libhwloc` by parsing the Linux `sysfs` and `procfs` hierarchies natively in Rust.

## Features
- **Hardware Locality:** Maps Sockets, L3 Caches, and Cores in a nested visual grid.
- **Zero Friction:** Runs unprivileged (Standard Sysfs) or elevated (eBPF-ready) with no external dependencies.
- **Microarchitectural Insights:** Color-codes cores based on execution state (Idle, Compute-Bound, etc.).
- **Static Portability:** Designed to be compiled as a single static binary for easy deployment over SSH.

## Getting Started

### Prerequisites
- Linux Kernel 4.x+
- Rust (Stable)

### Build
```bash
git clone https://github.com/swiftlogicsystems/swift-topomap.git
cd swift-topomap
cargo build --release
./target/release/swift-topomap

Project Roadmap

Phase 1: Native Sysfs Topology Resolver & Ratatui TUI.

Phase 2: eBPF CO-RE integration for LLC miss rates and IPC metrics.

Phase 3: NUMA distance mapping and memory bandwidth visualization.

About SwiftLogic Systems
SwiftLogic Systems specializes in high-performance systems engineering, low-latency observability, and kernel-level tooling.

Visit us at swiftlogic.systems.

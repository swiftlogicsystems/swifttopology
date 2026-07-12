# Technical Design: swift-topomap (ST-01)
**Author:** Ankur Rathore  
**Organization:** [SwiftLogic Systems](https://www.swiftlogic.systems)  
**Status:** Phase 2 (eBPF Integration)

---

## 1. Executive Summary
`swift-topomap` is a high-performance, zero-dependency hardware topology and telemetry mapper. It provides a real-time Terminal User Interface (TUI) that visualizes CPU hierarchies (Sockets, Caches, Cores) and overlays microarchitectural performance metrics using eBPF and the Linux Perf Subsystem.

## 2. Problem Statement
Standard tools like `lstopo` provide static snapshots of hardware but lack live execution context. Conversely, `top` or `htop` provide execution metrics but lack hardware awareness (e.g., "Are these two noisy processes sharing the same L3 cache?"). `swift-topomap` bridges this gap.

## 3. Architecture Overview
The system follows a "Deep-System" architecture, bypassing high-level wrappers to interact directly with the Linux Kernel ABI.

### 3.1 Topology Resolver (Native Sysfs)
Unlike legacy tools, `swift-topomap` uses a native Rust parser for `/sys/devices/system/cpu`. This ensures:
- **Zero dynamic dependencies:** No requirement for `libhwloc`.
- **Portability:** Works on any Linux distribution with a modern kernel.
- **Accuracy:** Maps physical package IDs to logical cores and shared L3 cache boundaries.

### 3.2 Hybrid Telemetry Engine (Trait-bound)
The tool utilizes a dynamic collection pipeline:
- **Standard Mode (Sysfs):** Parsed `/proc/stat` for unprivileged CPU utilization.
- **Advanced Mode (eBPF):** Loads a CO-RE (Compile Once – Run Everywhere) eBPF program to track context switches and hardware performance counters (LLC misses, IPC).

## 4. Phase 2: eBPF Implementation
Phase 2 introduces the `EbpfCollector`.

### 4.1 Kernel Logic
Using `libbpf`, the tool attaches to `tracepoint/sched/sched_switch`. 
- **Metrics collected:** Scheduler wait times, voluntary vs. involuntary context switches.
- **Hardware Counters:** Utilizing `BPF_MAP_TYPE_PERF_EVENT_ARRAY` to read Last Level Cache (LLC) misses directly from the PMU (Performance Monitoring Unit).

### 4.2 Build Pipeline
- **BTF Extraction:** Utilizes `bpftool` to generate `vmlinux.h`.
- **Skeleton Generation:** `libbpf-cargo` translates the C-eBPF bytecode into a safe Rust skeleton.

## 5. Strategic Value
`swift-topomap` serves as a high-authority proof of work for **SwiftLogic Systems**. It demonstrates mastery of Rust FFI, Linux Kernel internals, and low-latency UI design, providing a zero-friction entry point for future commercial offerings.

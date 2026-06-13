# Technical Design Document: SwiftTopology
**Project Code:** ST-01  
**Lead Organization:** SwiftLogic Systems  
**Architecture Style:** Phased — TUI Topology Viewer → eBPF Performance Overlay

---

## 1. Vision & Objectives

SwiftTopology is a terminal-native hardware topology viewer written in Rust, designed as an interactive successor to `lstopo`. It visualises the hardware tree (CPU packages, NUMA nodes, caches, cores, threads) in a navigable TUI and — in a second phase — overlays live performance metrics (IPC, cache miss rates, thread migration) sourced from eBPF.

### Why not just use lstopo?
`lstopo` is a static snapshot tool. It cannot show live performance data alongside topology, has no keyboard-driven navigation, and its terminal output is not interactive. SwiftTopology fills that gap.

### Goals by Phase

| Goal | Phase |
|---|---|
| Discover and display full hardware topology tree | 1 |
| Interactive keyboard navigation (expand/collapse nodes) | 1 |
| Detail panel (cache sizes, NUMA distances, CPU flags) | 1 |
| Runs without root, without kernel headers, on any Linux | 1 |
| Overlay live IPC, cache miss rate per core | 2 |
| Overlay thread migration heatmap | 2 |
| eBPF + perf_event counter collection | 2 |

---

## 2. System Architecture

```
┌─────────────────────────────────────────────────────┐
│                   SwiftTopology                     │
│                                                     │
│  ┌──────────────┐      ┌────────────────────────┐  │
│  │  Topology    │      │   Ratatui TUI          │  │
│  │  Model       │─────▶│   - Tree view          │  │
│  │  (hwloc2)    │      │   - Detail panel       │  │
│  └──────────────┘      │   - Metrics overlay    │  │
│                        └────────────────────────┘  │
│  ┌──────────────┐              ▲                    │
│  │  Metrics     │──────────────┘                   │
│  │  Collector   │   (Phase 2 only)                 │
│  │  (eBPF)      │                                  │
│  └──────────────┘                                  │
└─────────────────────────────────────────────────────┘
```

Phase 1 is the top half only. Phase 2 adds the metrics collector and wires it into the existing TUI.

---

## 3. Phase 1: lstopo-equivalent TUI

### 3.1 Topology Discovery (`src/topology.rs`)

Use the `hwloc2` crate (bindings to `libhwloc`) to walk the hardware object tree and build an owned Rust data model. This runs entirely in user-space with no elevated privileges.

**Key hwloc object types used:**

| hwloc Type | Represents |
|---|---|
| `ObjectType::Machine` | Root — the whole system |
| `ObjectType::Package` | Physical CPU socket |
| `ObjectType::NUMANode` | NUMA memory domain |
| `ObjectType::L3Cache` / `L2Cache` / `L1Cache` | Cache levels |
| `ObjectType::Core` | Physical core |
| `ObjectType::PU` | Logical processor (hardware thread) |

```rust
// src/topology.rs
use hwloc2::{Topology, TopologyObject, ObjectType};

#[derive(Debug, Clone)]
pub struct TopoNode {
    pub kind:     ObjectType,
    pub logical_index: u32,
    pub os_index: Option<u32>,
    pub label:    String,       // e.g. "L3 Cache (12 MiB)"
    pub detail:   Vec<String>,  // extra info shown in the detail panel
    pub children: Vec<TopoNode>,
}

pub fn build_tree() -> anyhow::Result<TopoNode> {
    let topo = Topology::new()?;
    let root = topo.object_at_root();
    Ok(walk(root))
}

fn walk(obj: &TopologyObject) -> TopoNode {
    let label = format_label(obj);
    let detail = format_detail(obj);
    let children = obj.children().map(walk).collect();
    TopoNode {
        kind: obj.object_type(),
        logical_index: obj.logical_index(),
        os_index: obj.os_index(),
        label,
        detail,
        children,
    }
}
```

### 3.2 Application State (`src/app.rs`)

Holds the topology tree and all UI state. The event loop mutates this struct and passes it to the renderer each frame.

```rust
// src/app.rs
use crate::topology::TopoNode;

pub struct App {
    pub tree:       TopoNode,
    // Flattened list of visible nodes for cursor tracking
    pub visible:    Vec<NodeRef>,
    pub cursor:     usize,
    pub expanded:   std::collections::HashSet<NodePath>,
    pub show_help:  bool,
    pub should_quit: bool,
}

impl App {
    pub fn new(tree: TopoNode) -> Self { /* ... */ }
    pub fn toggle_expand(&mut self) { /* expand/collapse at cursor */ }
    pub fn move_up(&mut self)   { self.cursor = self.cursor.saturating_sub(1); }
    pub fn move_down(&mut self) { self.cursor = (self.cursor + 1).min(self.visible.len() - 1); }
}
```

### 3.3 TUI Layout (`src/ui/mod.rs`)

Two-panel layout: the tree on the left, details on the right. A status bar at the bottom shows keybindings.

```
┌──────────────────────────┬─────────────────────────┐
│  Topology Tree           │  Details                │
│                          │                         │
│  ▼ Machine               │  L3 Cache               │
│    ▼ Package #0          │  ─────────────────────  │
│      ▼ L3 Cache          │  Size:      12 MiB      │
│        ▶ Core #0    ◀    │  Line size: 64 B        │
│        ▶ Core #1         │  Associat.: 16-way      │
│        ▶ Core #2         │  Shared by: 8 cores     │
│      NUMANode #0         │                         │
├──────────────────────────┴─────────────────────────┤
│  ↑↓ navigate   ↵ expand   q quit   ? help          │
└────────────────────────────────────────────────────┘
```

```rust
// src/ui/mod.rs
use ratatui::{Frame, layout::{Constraint, Direction, Layout}};
use crate::app::App;

pub fn draw(f: &mut Frame, app: &App) {
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(f.area());

    let main = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(outer[0]);

    draw_tree(f, app, main[0]);
    draw_detail(f, app, main[1]);
    draw_statusbar(f, outer[1]);
}
```

### 3.4 Event Loop (`src/main.rs`)

A simple synchronous loop — no async runtime needed for Phase 1.

```rust
// src/main.rs
use crossterm::event::{self, Event, KeyCode};
use ratatui::DefaultTerminal;
use crate::{app::App, topology, ui};

fn main() -> anyhow::Result<()> {
    let tree = topology::build_tree()?;
    let mut app = App::new(tree);
    let mut terminal = ratatui::init();

    loop {
        terminal.draw(|f| ui::draw(f, &app))?;

        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Char('q') => break,
                KeyCode::Up        => app.move_up(),
                KeyCode::Down      => app.move_down(),
                KeyCode::Enter | KeyCode::Char(' ') => app.toggle_expand(),
                KeyCode::Char('?') => app.show_help = !app.show_help,
                _ => {}
            }
        }
    }

    ratatui::restore();
    Ok(())
}
```

### 3.5 Phase 1 Dependencies

```toml
[dependencies]
hwloc2   = "2.5"
ratatui  = { version = "0.29", features = ["crossterm"] }
crossterm = "0.28"
anyhow   = "1.0"

# tokio is NOT needed for Phase 1
```

> **Note:** `libhwloc` must be installed on the system (`libhwloc-dev` on Debian/Ubuntu, `hwloc-devel` on Fedora). No kernel headers, no root access, no eBPF toolchain required.

---

## 4. Phase 2: eBPF Performance Overlay

Phase 2 extends the existing TUI by adding a metrics collector that runs alongside the render loop. The topology model and UI layout from Phase 1 are unchanged — metrics are simply an additional data source that the detail panel and tree decorations can read from.

### 4.1 What lstopo is Missing (and What We Add)

| Capability | lstopo | SwiftTopology Phase 2 |
|---|---|---|
| Hardware topology tree | ✅ | ✅ |
| Cache sizes, NUMA distances | ✅ | ✅ |
| Live IPC per core | ❌ | ✅ |
| Live L3 cache miss rate per core | ❌ | ✅ |
| Thread migration heatmap | ❌ | ✅ |
| Which PID is running on which core | ❌ | ✅ |

### 4.2 Kernel Layer (eBPF / C) (`src/bpf/swifttopology.bpf.c`)

Two BPF program types are needed: a tracepoint for scheduler events (thread migration, PID tracking) and a `perf_event` program for hardware counter sampling.

**Header generation:**
```bash
bpftool btf dump file /sys/kernel/btf/vmlinux format c > src/bpf/vmlinux.h
```

```c
#include "vmlinux.h"
#include <bpf/bpf_helpers.h>
#include <bpf/bpf_tracing.h>
#include <bpf/bpf_perf_event.h>

struct cpu_stats {
    __u64 total_cycles;
    __u64 instructions;
    __u64 cache_misses;
    __u32 current_pid;
    __u32 migrations;
};

struct {
    __uint(type, BPF_MAP_TYPE_PERCPU_ARRAY);
    __uint(max_entries, 256);
    __type(key, u32);
    __type(value, struct cpu_stats);
} cpu_data_map SEC(".maps");

// Tracks which PID is running and counts migrations
SEC("tracepoint/sched/sched_switch")
int handle_sched_switch(struct trace_event_raw_sched_switch *ctx) {
    u32 cpu_id = bpf_get_smp_processor_id();
    struct cpu_stats *stats = bpf_map_lookup_elem(&cpu_data_map, &cpu_id);
    if (stats) {
        if (stats->current_pid != 0 && stats->current_pid != ctx->next_pid)
            stats->migrations++;
        stats->current_pid = ctx->next_pid;
    }
    return 0;
}

// Samples hardware performance counters on perf_event overflow
SEC("perf_event")
int handle_perf_event(struct bpf_perf_event_data *ctx) {
    u32 cpu_id = bpf_get_smp_processor_id();
    struct cpu_stats *stats = bpf_map_lookup_elem(&cpu_data_map, &cpu_id);
    if (!stats)
        return 0;
    struct bpf_perf_event_value val;
    if (bpf_perf_event_read_value(&cpu_data_map, cpu_id, &val, sizeof(val)) == 0)
        stats->total_cycles = val.counter;
    // Instructions and cache misses are populated by separate perf_event fds
    // (one BPF program attached to each counter fd in user-space)
    return 0;
}

char LICENSE[] SEC("license") = "GPL";
```

### 4.3 Bridge Layer (`src/bpf_loader.rs`)

Loads the BPF skeleton, opens `perf_event` fds for cycles, instructions, and L3 cache misses on every CPU, and attaches the `perf_event` BPF program to each. Uses `libbpf-rs >= 0.23` which provides an owned skeleton API (no `'static` lifetime hack needed).

```rust
use libbpf_rs::skel::{OpenSkel, SkelBuilder};
use perf_event_open_sys as perf;
use std::os::unix::io::OwnedFd;

mod swifttopology_bpf {
    include!(concat!(env!("OUT_DIR"), "/swifttopology.skel.rs"));
}
use swifttopology_bpf::*;

pub struct BpfManager {
    skel:      SwifttopologySkel,
    _perf_fds: Vec<OwnedFd>,   // keep fds alive for the process lifetime
}

impl BpfManager {
    pub fn load(num_cpus: usize) -> anyhow::Result<Self> {
        let mut skel = SwifttopologySkelBuilder::default().open()?.load()?;
        skel.attach()?;
        let perf_fds = Self::attach_perf_events(&skel, num_cpus)?;
        Ok(Self { skel, _perf_fds: perf_fds })
    }

    fn attach_perf_events(skel: &SwifttopologySkel, num_cpus: usize) -> anyhow::Result<Vec<OwnedFd>> {
        // PERF_TYPE_HARDWARE: cycles=0, instructions=1, cache-misses=5
        let hw_configs: &[u64] = &[0, 1, 5];
        let mut fds = Vec::new();
        for cpu in 0..num_cpus {
            for &config in hw_configs {
                let fd = open_hw_perf_event(config, cpu as i32)?;
                skel.progs().handle_perf_event().attach_perf_event(&fd)?;
                fds.push(fd);
            }
        }
        Ok(fds)
    }

    pub fn get_stats(&self, cpu_id: u32) -> anyhow::Result<CpuStats> {
        let mut val = CpuStats::default();
        if let Some(bytes) = self.skel
            .maps().cpu_data_map()
            .lookup_percpu(&cpu_id.to_ne_bytes(), libbpf_rs::MapFlags::ANY)?
            .and_then(|v| v.get(cpu_id as usize).cloned())
        {
            val = plain::from_bytes(&bytes).copied().unwrap_or_default();
        }
        Ok(val)
    }
}

fn open_hw_perf_event(config: u64, cpu: i32) -> anyhow::Result<OwnedFd> {
    let mut attr = perf::bindings::perf_event_attr::default();
    attr.type_   = perf::bindings::PERF_TYPE_HARDWARE;
    attr.config  = config;
    attr.set_disabled(1);
    attr.set_exclude_kernel(0);
    let fd = unsafe { perf::perf_event_open(&mut attr, -1, cpu, -1, 0) };
    if fd < 0 { anyhow::bail!("perf_event_open failed: {}", std::io::Error::last_os_error()); }
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}
```

### 4.4 Metrics Integration into the TUI

The `App` struct gains an optional `BpfManager`. The detail panel and tree node decorations check for live data and render it when available. This keeps Phase 1 fully functional without eBPF.

```rust
// src/app.rs (additions for Phase 2)
pub struct App {
    // ... Phase 1 fields unchanged ...
    pub metrics: Option<BpfManager>,  // None when running without root / Phase 1 mode
}
```

Tree node decoration example — cores show a mini IPC bar:
```
▶ Core #0  [IPC ████░░░░ 0.82]  [L3$ miss 1.2%]
```

### 4.5 Phase 2 Build Pipeline

1. **BTF extraction:** `bpftool btf dump file /sys/kernel/btf/vmlinux format c > src/bpf/vmlinux.h`
2. **Clang compilation:** `swifttopology.bpf.c` → BPF ELF (handled by `build.rs` via `libbpf-cargo`)
3. **Skeleton generation:** `libbpf-cargo` generates `swifttopology.skel.rs` from the ELF
4. **Rust compilation:** Cargo compiles user-space code, linking against static `libbpf`
5. **Result:** Single binary; Phase 2 features activate automatically when run as root

### 4.6 Phase 2 Additional Dependencies

```toml
[dependencies]
libbpf-rs          = "0.24"
perf-event-open-sys = "0.1"
plain              = "0.2"   # safe byte-to-struct casting for BPF map values

[build-dependencies]
libbpf-cargo = "0.24"
```

---

## 5. File Structure

```
swifttopology/
├── src/
│   ├── main.rs           # event loop, terminal init
│   ├── app.rs            # App state, cursor, expand/collapse
│   ├── topology.rs       # hwloc2 tree discovery → TopoNode model
│   ├── bpf_loader.rs     # Phase 2: BpfManager, perf_event setup
│   └── ui/
│       ├── mod.rs        # layout, draw() entry point
│       ├── tree.rs       # tree panel renderer
│       ├── detail.rs     # detail panel renderer
│       └── statusbar.rs  # keybinding hint bar
│   └── bpf/
│       ├── vmlinux.h     # generated — not committed to VCS
│       └── swifttopology.bpf.c
├── build.rs              # Phase 2: libbpf-cargo skeleton generation
├── Cargo.toml
└── DESIGN.md
```

> **Phase 1 build note:** `build.rs` and the `bpf/` directory are only required for Phase 2. For Phase 1, `build.rs` can be a no-op and `libbpf-cargo` removed from `[build-dependencies]`.

---

## 6. Strategic Value

- **Fills a real gap:** lstopo has no live performance data, no interactive navigation, and no modern terminal UI.
- **Phased delivery:** Phase 1 ships as a useful tool with zero kernel dependencies. Phase 2 adds depth without breaking Phase 1.
- **Demonstrates expertise:** Bridges `libhwloc` (C), Ratatui (Rust TUI), and eBPF — three distinct technical domains working in concert.

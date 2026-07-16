#include "vmlinux.h"
#include <bpf/bpf_helpers.h>
#include <bpf/bpf_tracing.h>
#include <bpf/bpf_core_read.h>

// This matches the struct we will use in Rust
struct cpu_stats {
    u64 instructions;
    u64 l3_misses;
    u32 current_pid;
};

// A Per-CPU array is the most efficient way to store core-local data
struct {
    __uint(type, BPF_MAP_TYPE_PERCPU_ARRAY);
    __uint(max_entries, 256); // Support systems up to 256 cores
    __type(key, u32);
    __type(value, struct cpu_stats);
} cpu_stats_map SEC(".maps");

// 1. Hook into the Scheduler to track which PID is running on which Core
SEC("tp/sched/sched_switch")
int handle_sched_switch(struct trace_event_raw_sched_switch *ctx) {
    u32 cpu_id = bpf_get_smp_processor_id();
    struct cpu_stats *stats = bpf_map_lookup_elem(&cpu_stats_map, &cpu_id);

    if (stats) {
        stats->current_pid = ctx->next_pid;
    }
    return 0;
}

// 2. Hook into Hardware Performance Counters (PMU)
// This will be triggered by the physical CPU every time a counter overflows
SEC("perf_event")
int on_perf_event(struct bpf_perf_event_data *ctx) {
    u32 cpu_id = bpf_get_smp_processor_id();
    struct cpu_stats *stats = bpf_map_lookup_elem(&cpu_stats_map, &cpu_id);

    if (!stats)
        return 0;


    return 0;
}

char LICENSE[] SEC("license") = "GPL";

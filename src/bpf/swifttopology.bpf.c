#include "vmlinux.h"
#include <bpf/bpf_helpers.h>
#include <bpf/bpf_tracing.h>
#include <bpf/bpf_core_read.h>

// This matches the struct we will use in Rust
struct cpu_stats {
    u64 instructions;
    u64 cycles;
    u64 l3_misses;
    u32 current_pid;
    u32 padding;
};

// A Per-CPU array is the most efficient way to store core-local data
struct {
    __uint(type, BPF_MAP_TYPE_PERCPU_ARRAY);
    __uint(max_entries, 256); // Support systems up to 256 cores
    __type(key, u32);
    __type(value, struct cpu_stats);
} cpu_stats_map SEC(".maps");

// These are used to read the hardware PMU registers
struct {
    __uint(type, BPF_MAP_TYPE_PERF_EVENT_ARRAY);
    __uint(key_size, sizeof(u32));
    __uint(value_size, sizeof(u32));
} perf_instructions SEC(".maps");

struct {
    __uint(type, BPF_MAP_TYPE_PERF_EVENT_ARRAY);
    __uint(key_size, sizeof(u32));
    __uint(value_size, sizeof(u32));
} perf_cycles SEC(".maps");

struct {
    __uint(type, BPF_MAP_TYPE_PERF_EVENT_ARRAY);
    __uint(key_size, sizeof(u32));
    __uint(value_size, sizeof(u32));
} perf_l3_misses SEC(".maps");

// 1. Hook into the Scheduler to track which PID is running on which Core
SEC("tp/sched/sched_switch")
int handle_sched_switch(struct trace_event_raw_sched_switch *ctx) {
    u32 cpu_id = bpf_get_smp_processor_id();
    struct cpu_stats *stats = bpf_map_lookup_elem(&cpu_stats_map, &cpu_id);
    if (!stats) return 0;

    stats->current_pid = ctx->next_pid;

    // Read the raw hardware counters
    stats->instructions = bpf_perf_event_read(&perf_instructions, cpu_id);
    stats->cycles = bpf_perf_event_read(&perf_cycles, cpu_id);
    stats->l3_misses = bpf_perf_event_read(&perf_l3_misses, cpu_id);

    return 0;
}

char LICENSE[] SEC("license") = "GPL";

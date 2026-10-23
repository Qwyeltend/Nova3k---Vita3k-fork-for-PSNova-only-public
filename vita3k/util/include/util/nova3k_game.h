// Nova3K: small per-game runtime flags shared between emulator modules.
// Set once when a game boots (vita3k/interface.cpp); false for every game Nova3K has no patches for.
#pragma once

#include <atomic>
#include <cstdint>

namespace nova3k::game {

// Phantasy Star Online 2: log the game's network activity (resolved host names, connections)
inline std::atomic<bool> &pso2_net_log() {
    static std::atomic<bool> enabled{ false };
    return enabled;
}

// Phantasy Star Online 2 with its patches on (Settings > CPU > Nova3K tweaks > PSO2 patches)
inline std::atomic<bool> &pso2_active() {
    static std::atomic<bool> enabled{ false };
    return enabled;
}

// Phantasy Star Online 2: the render thread notes the command it is working on, so a crash report
// (nova3k_crash.txt) can say which graphics command was running. Plain stores, read only by the crash handler.
struct RenderTrace {
    volatile uint64_t commands = 0; // commands processed so far (0: never filled, nothing to print)
    volatile uint32_t opcode = 0; // renderer::CommandOpcode of the current command
    volatile uint32_t flags = 0; // renderer::Command flags (1 = allocated on the host, 2 = never freed)
    volatile uintptr_t command = 0; // address of the command
    volatile uintptr_t context = 0; // renderer context of the command list
    volatile uint32_t stage = 0; // 1 = running the command, 2 = freeing it
};

inline RenderTrace &render_trace() {
    static RenderTrace trace;
    return trace;
}

// Phantasy Star Online 2: graphics counters, reported once a second next to "Nova3K perf" (only while PSO2 runs).
// Used to find out why models are missing in the character creator.
struct GfxCounters {
    std::atomic<uint64_t> draws{ 0 }; // sceGxmDraw / sceGxmDrawInstanced accepted
    std::atomic<uint64_t> draw_errors{ 0 }; // ... rejected with an error
    std::atomic<uint64_t> precomputed{ 0 }; // sceGxmDrawPrecomputed calls
    std::atomic<uint64_t> skipped_no_streams{ 0 }; // renderer: draw without vertex streams
    std::atomic<uint64_t> skipped_no_pipeline{ 0 }; // renderer: no Vulkan pipeline (shader not ready)
    std::atomic<uint64_t> vis_queries{ 0 }; // visibility (occlusion) queries started
    std::atomic<uint64_t> dirty_marks{ 0 }; // render target written by the CPU -> GPU copy treated as stale
    std::atomic<uint64_t> dirty_reads{ 0 }; // texture read from guest memory because its surface is stale
    std::atomic<uint64_t> dirty_clears{ 0 }; // stale surface rendered to again
    std::atomic<uint64_t> depth_tex_gpu{ 0 }; // depth buffer sampled as a texture, found on the GPU
    std::atomic<uint64_t> depth_tex_memory{ 0 }; // ... not found, read from guest memory instead
    std::atomic<uint64_t> depth_scenes_memory{ 0 }; // scenes whose depth buffer is kept (force load/store)
    std::atomic<uint64_t> depth_scenes_gpu_only{ 0 }; // scenes with a depth buffer the emulator does not keep
    std::atomic<uint64_t> depth_restart_cleared{ 0 }; // render pass restarted inside a scene with depth cleared again
    std::atomic<uint64_t> depth_restart_kept{ 0 }; // ... restarted with the depth loaded
    std::atomic<uint64_t> surface_reloads{ 0 }; // surface image reloaded from memory after a CPU transfer
    std::atomic<uint64_t> gpu_downscales{ 0 }; // sceGxmTransferDownscale done on the GPU (no readback)
    std::atomic<uint32_t> vis_buffer_address{ 0 }; // last visibility (occlusion query) buffer set by the game
    std::atomic<uint32_t> vis_buffer_entries{ 0 }; // ... its number of 32-bit entries
    std::atomic<uint64_t> vis_queries_increment{ 0 }; // queries counting visible samples
    std::atomic<uint64_t> vis_queries_set{ 0 }; // queries only telling visible / not visible
};

inline GfxCounters &gfx_counters() {
    static GfxCounters counters;
    return counters;
}

inline void gfx_count(std::atomic<uint64_t> GfxCounters::*counter, uint64_t n = 1) {
    if (pso2_active().load(std::memory_order_relaxed))
        (gfx_counters().*counter).fetch_add(n, std::memory_order_relaxed);
}

} // namespace nova3k::game

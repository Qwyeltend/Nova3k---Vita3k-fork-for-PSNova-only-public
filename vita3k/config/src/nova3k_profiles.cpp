// Nova3K: built-in per-game settings profiles. See config/nova3k_profiles.h
#include <config/nova3k_profiles.h>

#include <array>

namespace config::nova3k {

namespace {

// Phantasy Star Nova - the settings from the Nova3K README
void apply_ps_nova(Config::CurrentConfig &cfg) {
    cfg.backend_renderer = "Vulkan";
    cfg.v_sync = true;
    cfg.disable_surface_sync = false;
    cfg.async_pipeline_compilation = true;
    cfg.memory_mapping = "double-buffer";
    cfg.fps_hack = true; // 60 FPS
    cfg.audio_backend = "SDL";
}

const Profile ps_nova{ "Phantasy Star Nova", true, true, true, false, &apply_ps_nova };

// Phantasy Star Online 2 (Vita, final client 6.31). Work in progress: no settings are forced yet,
// only the PSO2 patches and the performance monitor (useful for bug reports) are switched on.
void apply_pso2(Config::CurrentConfig &) {}

const Profile pso2{ "Phantasy Star Online 2", false, false, true, true, &apply_pso2 };

struct Entry {
    std::string_view title_id;
    const Profile *profile;
};

constexpr std::array<Entry, 3> profiles{ {
    { "PCSG00351", &ps_nova }, // Japan
    { "PCSH00143", &ps_nova }, // Asia
    { "PCSG00141", &pso2 }, // Japan
} };

} // namespace

const Profile *find_profile(std::string_view title_id) {
    for (const auto &entry : profiles) {
        if (entry.title_id == title_id)
            return entry.profile;
    }
    return nullptr;
}

void apply_profile(Config::CurrentConfig &cfg, std::string_view title_id) {
    if (const Profile *profile = find_profile(title_id))
        profile->apply(cfg);
}

bool tweak_enabled(const Config::CurrentConfig &cfg, std::string_view title_id, Tweak tweak) {
    int mode = TWEAK_AUTO;
    switch (tweak) {
    case Tweak::LockFreeMonitor: mode = cfg.nova3k_lock_free_monitor; break;
    case Tweak::FastJitInit: mode = cfg.nova3k_fast_jit_init; break;
    case Tweak::PerfMonitor: mode = cfg.nova3k_perf_monitor; break;
    case Tweak::Pso2Patches: mode = cfg.nova3k_pso2_patches; break;
    }
    if (mode == TWEAK_ON)
        return true;
    if (mode == TWEAK_OFF)
        return false;

    const Profile *profile = find_profile(title_id);
    if (!profile)
        return false;
    switch (tweak) {
    case Tweak::LockFreeMonitor: return profile->lock_free_monitor;
    case Tweak::FastJitInit: return profile->fast_jit_init;
    case Tweak::PerfMonitor: return profile->perf_monitor;
    case Tweak::Pso2Patches: return profile->pso2_patches;
    }
    return false;
}

} // namespace config::nova3k

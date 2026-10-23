// Nova3K: built-in per-game settings profiles and per-game switches for Nova3K's own changes.
//
// A game listed here works out of the box: until the user saves their own per-game settings, the
// emulator uses the profile below instead of the global settings. Nova3K's changes to the emulator
// core ("tweaks") are switched per game too: Auto follows the profile (off for every other game),
// On / Off force them. Off always means the original Vita3K behaviour.
#pragma once

#include <config/state.h>

#include <string_view>

namespace config::nova3k {

enum TweakMode : int {
    TWEAK_AUTO = 0,
    TWEAK_ON = 1,
    TWEAK_OFF = 2,
};

enum class Tweak {
    LockFreeMonitor, // LDREX/STREX without dynarmic's global spinlock (big FPS gain in Nova)
    FastJitInit, // flush only the JIT prelude, not the whole code cache, when a guest thread starts
    PerfMonitor, // once-per-second performance line and slow-frame report in the log
    Pso2Patches, // Phantasy Star Online 2: built-in game patches and network log (see vita3k/nova3k/pso2.cpp)
};

struct Profile {
    const char *name; // shown in the log
    bool lock_free_monitor;
    bool fast_jit_init;
    bool perf_monitor;
    bool pso2_patches;
    void (*apply)(Config::CurrentConfig &cfg); // settings the game needs; everything else stays global
};

// nullptr if the game has no built-in profile
const Profile *find_profile(std::string_view title_id);

// Overwrites the settings the profile cares about. Does nothing for games without a profile.
void apply_profile(Config::CurrentConfig &cfg, std::string_view title_id);

// Resolves Auto / On / Off for this game
bool tweak_enabled(const Config::CurrentConfig &cfg, std::string_view title_id, Tweak tweak);

} // namespace config::nova3k

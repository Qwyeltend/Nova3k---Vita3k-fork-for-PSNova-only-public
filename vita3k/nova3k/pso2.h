// Nova3K: Phantasy Star Online 2 (PS Vita, PCSG00141) support.
// Everything here runs only for that title and only while Settings > CPU > Nova3K tweaks >
// "PSO2 patches" resolves to on (Auto = on for PSO2). Other games never reach this code.
#pragma once

#include <string_view>

struct EmuEnvState;
struct SceKernelModuleInfo;

namespace nova3k::pso2 {

constexpr std::string_view title_id = "PCSG00141";

// Called once for the game's main executable after it is loaded into memory
void apply_patches(EmuEnvState &emuenv, const SceKernelModuleInfo &info);

} // namespace nova3k::pso2

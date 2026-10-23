// Nova3K: built-in server for Phantasy Star Online 2 (PS Vita). Work in progress.
//
// The game's servers (gsNNN.pso2gs.net) are redirected to 127.0.0.1 (sceNetResolverStartNtoa in
// SceNet.cpp) and this server answers on the ports the game uses. Runs only for PSO2 with its patches on.
// Everything it receives and sends is written to vita3k.log ("Nova3K PSO2 server:").
//
// Packets: 8-byte header (u32 size including the header, u8 type, u8 subtype, u8 flags, u8 flags2),
// the same format as the PC version of PSO2 (the Vita client used the same servers).
//
// Ports (the Vita client uses its own numbers; PC equivalents in brackets):
//   12x94  ship list    -> 11-3D ship list, then close              (PC 12x99)
//   12x93  block balance-> 11-2C "go to block server 127.0.0.1:12205" (PC 12x00)
//   12205  block server -> 03-08 hello, 11-0B key exchange -> 11-0C, then RC4-encrypted traffic
//
// Builds with the PhantasyServer library (NOVA3K_PSO2_PHANTASY, Android CI) run PhantasyServer by
// Anton Maltsev (MIT license) instead: a complete PSO2 server with lobby, quests and enemies, see
// external/phantasyserver. The small server below is then not started.
#pragma once

#include <string>

namespace nova3k::pso2::server {

// Starts the server once; later calls do nothing. vita_dir: the emulator's Vita folder (server data
// goes to <vita_dir>/pso2_server).
void start(const std::string &vita_dir);

} // namespace nova3k::pso2::server

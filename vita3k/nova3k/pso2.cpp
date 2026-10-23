// Nova3K: Phantasy Star Online 2 (PS Vita, PCSG00141) support. See pso2.h
//
// Addresses were found by static analysis of the 6.31 executable; each patch checks the original bytes first.
#include "nova3k/pso2.h"
#include "nova3k/pso2_key.h"

#include <emuenv/state.h>
#include <io/state.h>
#include <kernel/state.h>
#include <kernel/types.h>
#include <mem/ptr.h>
#include <mem/state.h>
#include <util/log.h>

#include <array>
#include <vector>
#include <cstring>

namespace nova3k::pso2 {

namespace {

// A patch replaces `patched` with `replacement` at `offset` in segment 0 (code), but only if the
// bytes around it (`expected`, starting `check_offset`) match the known 6.31 executable exactly.
// Any other version of the game is left untouched instead of being corrupted.
struct CodePatch {
    const char *name;
    uint32_t check_offset;
    std::vector<uint8_t> expected;
    uint32_t offset;
    std::vector<uint8_t> replacement;
};

// Title menu, item 0 "Game Start" (ゲームスタート).
// In 6.31 the title menu's input handler (0x825ca3be) jumps for item 0 to code that only opens the
// "PS Vita service ended" notice (VitaEndTitle / VitaEndText) and returns. For every other item it
// falls through to 0x825ca97e, which passes the item to the login sequence (callback 0x82fc5d66);
// there item 0 still leads to the online login (Connecting, Ini.GameServer etc.).
// The patch turns `bne.w 0x825ca97e` after `cmp r5, #0` into an unconditional `b.w 0x825ca97e`,
// so Game Start goes the same way as the other items.
const CodePatch game_start{
    "Game Start (skip the service-ended notice)",
    0x15CA64A, // 0x825ca64a: cmp r5,#0 / bne.w 0x825ca97e / movw r0,#0x51a8
    { 0x00, 0x2D, 0x40, 0xF0, 0x97, 0x81, 0x45, 0xF2, 0xA8, 0x10 }, // bytes as stored (little-endian halfwords)
    0x15CA64C,
    { 0x00, 0xF0, 0x97, 0xB9 }, // b.w 0x825ca97e
};

// Server key: the game encrypts its session key with an RSA-1024 public key stored as a Windows
// PUBLICKEYBLOB at 0x83d68ce8 (148 bytes, used by 0x82ef2106). It is replaced with the built-in
// server's key (nova3k/pso2_key.h), so the built-in server can read the key exchange packet 11-0B.
const CodePatch server_key{
    "server key (built-in server)",
    0x2D68CE8, // header 06 02 00 00 00 A4 00 00 "RSA1", 1024 bits, e = 65537, first modulus bytes
    { 0x06, 0x02, 0x00, 0x00, 0x00, 0xA4, 0x00, 0x00, 0x52, 0x53, 0x41, 0x31, 0x00, 0x04, 0x00, 0x00,
        0x01, 0x00, 0x01, 0x00, 0x4D, 0x06, 0x14, 0x64 },
    0x2D68CE8,
    std::vector<uint8_t>(key::public_blob.begin(), key::public_blob.end()),
};

bool apply(EmuEnvState &emuenv, const SceKernelModuleInfo &info, const CodePatch &patch) {
    const auto &seg = info.segments[0];
    const auto seg_ptr = seg.vaddr.cast<uint8_t>();
    if (!seg_ptr || patch.check_offset + patch.expected.size() > seg.memsz || patch.offset + patch.replacement.size() > seg.memsz) {
        LOG_WARN("Nova3K PSO2: unexpected executable layout, patch \"{}\" skipped", patch.name);
        return false;
    }
    uint8_t *code = seg_ptr.get(emuenv.mem);
    if (memcmp(code + patch.check_offset, patch.expected.data(), patch.expected.size()) != 0) {
        LOG_WARN("Nova3K PSO2: game version differs from 6.31, patch \"{}\" skipped", patch.name);
        return false;
    }
    memcpy(code + patch.offset, patch.replacement.data(), patch.replacement.size());
    LOG_INFO("Nova3K PSO2: patch \"{}\" applied", patch.name);
    return true;
}

} // namespace

void apply_patches(EmuEnvState &emuenv, const SceKernelModuleInfo &info) {
    if (emuenv.io.title_id != title_id)
        return;
    apply(emuenv, info, game_start);
    apply(emuenv, info, server_key);
}

} // namespace nova3k::pso2

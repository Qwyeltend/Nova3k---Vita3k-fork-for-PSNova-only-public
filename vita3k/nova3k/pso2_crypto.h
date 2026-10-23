// Nova3K: the two ciphers of the PSO2 protocol, small enough to keep here instead of adding a crypto library.
// - RSA-1024 private-key decryption with PKCS#1 v1.5 unpadding (the game's key exchange packet 11-0B)
// - RC4 (everything after the key exchange is RC4-encrypted in both directions)
#pragma once

#include <array>
#include <cstdint>
#include <optional>
#include <vector>

namespace nova3k::pso2::crypto {

// m = c^d mod n for 1024-bit numbers, all big-endian 128-byte arrays
std::array<uint8_t, 128> rsa_modexp(const std::array<uint8_t, 128> &c, const std::array<uint8_t, 128> &d, const std::array<uint8_t, 128> &n);

// Decrypts a PKCS#1 v1.5 (type 2) block with the built-in server key; nullopt if the padding is wrong
std::optional<std::vector<uint8_t>> rsa_decrypt(const std::array<uint8_t, 128> &ciphertext_big_endian);

class Rc4 {
public:
    Rc4() = default;
    explicit Rc4(const uint8_t *key, size_t key_size);
    void apply(uint8_t *data, size_t size); // encrypts or decrypts in place

private:
    std::array<uint8_t, 256> s{};
    uint8_t i = 0, j = 0;
};

} // namespace nova3k::pso2::crypto

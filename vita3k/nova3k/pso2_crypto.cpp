// Nova3K: RSA-1024 decryption and RC4 for the built-in PSO2 server. See pso2_crypto.h
#include "nova3k/pso2_crypto.h"

#include "nova3k/pso2_key.h"

#include <cstring>

namespace nova3k::pso2::crypto {

namespace {

constexpr int limbs = 32; // 1024 bits in 32-bit words, least significant first
using Num = std::array<uint32_t, limbs>;

Num from_bytes(const std::array<uint8_t, 128> &b) {
    Num n{};
    for (int i = 0; i < limbs; ++i) {
        const int o = 128 - 4 * (i + 1);
        n[i] = (uint32_t(b[o]) << 24) | (uint32_t(b[o + 1]) << 16) | (uint32_t(b[o + 2]) << 8) | b[o + 3];
    }
    return n;
}

std::array<uint8_t, 128> to_bytes(const Num &n) {
    std::array<uint8_t, 128> b{};
    for (int i = 0; i < limbs; ++i) {
        const int o = 128 - 4 * (i + 1);
        b[o] = uint8_t(n[i] >> 24);
        b[o + 1] = uint8_t(n[i] >> 16);
        b[o + 2] = uint8_t(n[i] >> 8);
        b[o + 3] = uint8_t(n[i]);
    }
    return b;
}

bool greater_or_equal(const uint32_t *a, const Num &b, uint32_t a_top) {
    if (a_top)
        return true;
    for (int i = limbs - 1; i >= 0; --i) {
        if (a[i] != b[i])
            return a[i] > b[i];
    }
    return true;
}

void subtract(uint32_t *a, const Num &b) {
    uint64_t borrow = 0;
    for (int i = 0; i < limbs; ++i) {
        const uint64_t d = uint64_t(a[i]) - b[i] - borrow;
        a[i] = uint32_t(d);
        borrow = (d >> 63) & 1;
    }
}

// Montgomery multiplication (CIOS): returns a * b * R^-1 mod n, R = 2^1024
Num mont_mul(const Num &a, const Num &b, const Num &n, uint32_t n0_inv) {
    uint32_t t[limbs + 2] = {};
    for (int i = 0; i < limbs; ++i) {
        uint64_t carry = 0;
        for (int j = 0; j < limbs; ++j) {
            const uint64_t s = uint64_t(t[j]) + uint64_t(a[j]) * b[i] + carry;
            t[j] = uint32_t(s);
            carry = s >> 32;
        }
        uint64_t s = uint64_t(t[limbs]) + carry;
        t[limbs] = uint32_t(s);
        t[limbs + 1] = uint32_t(s >> 32);

        const uint32_t m = t[0] * n0_inv;
        s = uint64_t(t[0]) + uint64_t(m) * n[0];
        carry = s >> 32;
        for (int j = 1; j < limbs; ++j) {
            s = uint64_t(t[j]) + uint64_t(m) * n[j] + carry;
            t[j - 1] = uint32_t(s);
            carry = s >> 32;
        }
        s = uint64_t(t[limbs]) + carry;
        t[limbs - 1] = uint32_t(s);
        t[limbs] = t[limbs + 1] + uint32_t(s >> 32);
        t[limbs + 1] = 0;
    }
    if (greater_or_equal(t, n, t[limbs]))
        subtract(t, n);
    Num r{};
    std::memcpy(r.data(), t, sizeof(r));
    return r;
}

// R^2 mod n by doubling 1 (2 * 1024) times with modular reduction
Num r_squared(const Num &n) {
    uint32_t x[limbs + 1] = { 1 };
    for (int bit = 0; bit < 2 * 32 * limbs; ++bit) {
        uint32_t carry = 0;
        for (int i = 0; i <= limbs; ++i) {
            const uint32_t next = x[i] >> 31;
            x[i] = (x[i] << 1) | carry;
            carry = next;
        }
        if (greater_or_equal(x, n, x[limbs])) {
            subtract(x, n);
            x[limbs] = 0;
        }
    }
    Num r{};
    std::memcpy(r.data(), x, sizeof(r));
    return r;
}

} // namespace

std::array<uint8_t, 128> rsa_modexp(const std::array<uint8_t, 128> &c_bytes, const std::array<uint8_t, 128> &d_bytes, const std::array<uint8_t, 128> &n_bytes) {
    const Num n = from_bytes(n_bytes);
    const Num c = from_bytes(c_bytes);
    // -n^-1 mod 2^32 (n is odd)
    uint32_t inv = 1;
    for (int k = 0; k < 5; ++k)
        inv *= 2 - n[0] * inv;
    const uint32_t n0_inv = uint32_t(0) - inv;

    const Num rr = r_squared(n);
    Num one{};
    one[0] = 1;
    Num result = mont_mul(one, rr, n, n0_inv); // R mod n = Montgomery form of 1
    const Num base = mont_mul(c, rr, n, n0_inv);
    for (int i = 0; i < 128; ++i) {
        for (int bit = 7; bit >= 0; --bit) {
            result = mont_mul(result, result, n, n0_inv);
            if ((d_bytes[i] >> bit) & 1)
                result = mont_mul(result, base, n, n0_inv);
        }
    }
    return to_bytes(mont_mul(result, one, n, n0_inv));
}

std::optional<std::vector<uint8_t>> rsa_decrypt(const std::array<uint8_t, 128> &ciphertext) {
    const auto m = rsa_modexp(ciphertext, key::private_exponent, key::modulus);
    // 00 02 <non-zero padding, at least 8 bytes> 00 <data>
    if (m[0] != 0x00 || m[1] != 0x02)
        return std::nullopt;
    size_t i = 2;
    while (i < m.size() && m[i] != 0)
        ++i;
    if (i >= m.size() || i < 10)
        return std::nullopt;
    return std::vector<uint8_t>(m.begin() + i + 1, m.end());
}

Rc4::Rc4(const uint8_t *key, size_t key_size) {
    for (int k = 0; k < 256; ++k)
        s[k] = uint8_t(k);
    uint8_t jj = 0;
    for (int k = 0; k < 256; ++k) {
        jj = uint8_t(jj + s[k] + key[k % key_size]);
        std::swap(s[k], s[jj]);
    }
}

void Rc4::apply(uint8_t *data, size_t size) {
    for (size_t k = 0; k < size; ++k) {
        i = uint8_t(i + 1);
        j = uint8_t(j + s[i]);
        std::swap(s[i], s[j]);
        data[k] ^= s[uint8_t(s[i] + s[j])];
    }
}

} // namespace nova3k::pso2::crypto

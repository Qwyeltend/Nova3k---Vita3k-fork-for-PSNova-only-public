// Nova3K: built-in server for Phantasy Star Online 2 (PS Vita). See pso2_server.h
//
// Packet layouts follow what the PC client community documented (e.g. the PolarisServer project);
// the Vita client is checked against them one step at a time through the log.
#include "nova3k/pso2_server.h"

#include "nova3k/pso2_crypto.h"

#include <util/log.h>

#include <atomic>
#include <chrono>
#include <cstring>
#include <ctime>
#include <memory>
#include <string>
#include <thread>
#include <vector>

#ifndef _WIN32
#include <arpa/inet.h>
#include <cerrno>
#include <netinet/in.h>
#include <sys/select.h>
#include <sys/socket.h>
#include <unistd.h>
#endif

namespace nova3k::pso2::server {

#ifdef NOVA3K_PSO2_PHANTASY

extern "C" {
typedef void (*nova3k_pso2_log_fn)(int level, const char *message);
int nova3k_pso2_server_start(const char *dir, nova3k_pso2_log_fn log);
}

static void phantasy_log(int level, const char *message) {
    switch (level) {
    case 0: LOG_ERROR("PhantasyServer: {}", message); break;
    case 1: LOG_WARN("PhantasyServer: {}", message); break;
    default: LOG_INFO("PhantasyServer: {}", message); break;
    }
}

void start(const std::string &vita_dir) {
    const std::string dir = vita_dir + "/pso2_server";
    LOG_INFO("Nova3K PSO2: starting PhantasyServer (c) Anton Maltsev, MIT license - thank you! Data: {}", dir);
    const int result = nova3k_pso2_server_start(dir.c_str(), &phantasy_log);
    if (result < 0)
        LOG_ERROR("Nova3K PSO2: PhantasyServer did not start (code {})", result);
}

#elif defined(_WIN32)

void start(const std::string &) {
    LOG_WARN("Nova3K PSO2 server: not available on Windows yet");
}

#else

namespace {

constexpr uint16_t block_port = 12205;

enum class Role {
    ShipList,
    BlockBalance,
    Block,
};

const char *role_name(Role role) {
    switch (role) {
    case Role::ShipList: return "ship list";
    case Role::BlockBalance: return "block balance";
    case Role::Block: return "block";
    }
    return "?";
}

std::string hex_dump(const uint8_t *data, size_t size, size_t limit = 256) {
    std::string out;
    char buf[4];
    const size_t shown = std::min(size, limit);
    for (size_t i = 0; i < shown; ++i) {
        snprintf(buf, sizeof(buf), "%02X", data[i]);
        if (i)
            out += (i % 16 == 0) ? " | " : " ";
        out += buf;
    }
    if (size > shown)
        out += " ... (" + std::to_string(size) + " bytes)";
    return out;
}

// Little-endian packet builder
class Writer {
public:
    void u8(uint8_t v) { data.push_back(v); }
    void u16(uint16_t v) {
        u8(uint8_t(v));
        u8(uint8_t(v >> 8));
    }
    void u32(uint32_t v) {
        u16(uint16_t(v));
        u16(uint16_t(v >> 16));
    }
    void bytes(const uint8_t *p, size_t n) { data.insert(data.end(), p, p + n); }
    void zeros(size_t n) { data.insert(data.end(), n, 0); }
    // PSO2 "magic" count of a variable-length array: (value + sub) ^ xor
    void magic(uint32_t value, uint32_t xor_key, uint32_t sub) { u32((value + sub) ^ xor_key); }
    // fixed-size UTF-16 string (ASCII text only)
    void utf16(const std::string &s, size_t chars) {
        for (size_t i = 0; i < chars; ++i)
            u16(i < s.size() ? uint16_t(uint8_t(s[i])) : 0);
    }
    std::vector<uint8_t> data;
};

std::vector<uint8_t> packet(uint8_t type, uint8_t subtype, uint8_t flags, const std::vector<uint8_t> &payload) {
    Writer w;
    w.u32(uint32_t(payload.size() + 8));
    w.u8(type);
    w.u8(subtype);
    w.u8(flags);
    w.u8(0);
    w.bytes(payload.data(), payload.size());
    return w.data;
}

// 11-3D ship list: magic count, 0x34-byte entries, server time, 1
std::vector<uint8_t> ship_list_packet() {
    Writer w;
    constexpr int ships = 10;
    w.magic(ships, 0xE418, 81);
    for (int i = 1; i <= ships; ++i) {
        w.u32(uint32_t(i)); // ship number
        w.utf16(i == 1 ? "Nova3K" : "Ship" + std::string(i < 10 ? "0" : "") + std::to_string(i), 16);
        const uint8_t ip[4] = { 127, 0, 0, 1 };
        w.bytes(ip, 4);
        w.u32(0);
        w.u16(i == 1 ? 1 : 4); // status: 1 online, 4 offline
        w.u16(uint16_t(i)); // order
        w.u32(0);
    }
    w.u32(uint32_t(std::time(nullptr)));
    w.u32(1);
    return packet(0x11, 0x3D, 0x04, w.data);
}

// 11-2C block balance: 0x90 bytes, block server address at 0x68 and port at 0x6C
std::vector<uint8_t> block_balance_packet() {
    Writer w;
    w.zeros(0x68 - 8);
    const uint8_t ip[4] = { 127, 0, 0, 1 };
    w.bytes(ip, 4);
    w.u16(block_port);
    w.zeros(0x90 - 0x6E);
    return packet(0x11, 0x2C, 0x00, w.data);
}

// 03-08 hello, sent by the block server right after the game connects
std::vector<uint8_t> hello_packet() {
    Writer w;
    w.u16(3);
    w.u16(201);
    w.u16(0);
    w.u16(0);
    return packet(0x03, 0x08, 0x00, w.data);
}

constexpr uint32_t player_id = 1;

// 11-01 login answer (PC layout): status 0 = OK, error text (empty), player object, block name, then
// values the PC community has not identified yet (one of them is the level cap); copied as documented.
std::vector<uint8_t> login_answer_packet() {
    Writer w;
    w.u32(0); // status: 0 OK
    w.magic(0, 0x8BA4, 0xB6); // error message: empty UTF-16 string
    w.u32(player_id); // object header: id, padding, entity type 4 (player), unknown
    w.u32(0);
    w.u16(4);
    w.u16(0);
    w.utf16("Nova3K Block 1", 0x20);
    const uint32_t values[] = { 0x42700000, 7, 0xA, 1, 0x41200000, 0x40A00000, 11, 0x3F800000, 0x42960000, 40, 0x41200000, 1, 1120403456 };
    for (const uint32_t v : values)
        w.u32(v);
    for (int i = 0; i < 10; ++i)
        w.u32(1065353216);
    for (int i = 0; i < 21; ++i)
        w.u32(1120403456);
    w.u32(0x91A2B);
    w.u32(0x91A2B);
    w.zeros(12);
    return packet(0x11, 0x01, 0x04, w.data);
}

// 11-03 character list: count, 4 zero bytes, characters. Empty for now: the game should open character creation.
std::vector<uint8_t> character_list_packet() {
    Writer w;
    w.u32(0);
    w.u32(0);
    return packet(0x11, 0x03, 0x00, w.data);
}

struct Client {
    int fd = -1;
    int port = 0;
    Role role = Role::Block;
    std::chrono::steady_clock::time_point last_data;
    size_t received = 0;
    bool reported_silence = false;
    bool close_after_send = false;
    std::vector<uint8_t> in; // decrypted, not yet parsed
    std::unique_ptr<crypto::Rc4> rx, tx; // set after the key exchange
};

void send_packet(Client &client, std::vector<uint8_t> data, const char *what) {
    LOG_INFO("Nova3K PSO2 server: [{} {}] send {:02X}-{:02X} {} ({} bytes): {}", role_name(client.role), client.port, data[4], data[5], what,
        data.size(), hex_dump(data.data(), data.size(), 64));
    if (client.tx)
        client.tx->apply(data.data(), data.size());
    size_t sent = 0;
    while (sent < data.size()) {
        const ssize_t n = send(client.fd, data.data() + sent, data.size() - sent, MSG_NOSIGNAL);
        if (n <= 0) {
            LOG_WARN("Nova3K PSO2 server: send failed ({})", strerror(errno));
            return;
        }
        sent += size_t(n);
    }
}

// 11-0B key exchange: 0x80 bytes RSA-encrypted (byte order reversed, as Windows CryptoAPI writes it).
// Inside: 0x10 bytes token + 0x10 bytes RC4 key. Answer 11-0C with the token decrypted by the RC4 key.
void handle_key_exchange(Client &client, const uint8_t *payload, size_t size) {
    if (size < 0x80) {
        LOG_WARN("Nova3K PSO2 server: key exchange too short ({} bytes)", size);
        return;
    }
    std::array<uint8_t, 128> blob{};
    for (size_t i = 0; i < 0x80; ++i)
        blob[i] = payload[0x7F - i];
    const auto plain = crypto::rsa_decrypt(blob);
    if (!plain || plain->size() < 0x20) {
        LOG_WARN("Nova3K PSO2 server: key exchange could not be decrypted (the game is not using the built-in key?)");
        return;
    }
    LOG_INFO("Nova3K PSO2 server: key exchange decrypted ({} bytes): {}", plain->size(), hex_dump(plain->data(), plain->size()));
    const uint8_t *rc4_key = plain->data() + 0x10;
    std::vector<uint8_t> token(plain->begin(), plain->begin() + 0x10);
    crypto::Rc4(rc4_key, 0x10).apply(token.data(), token.size());

    // RC4 starts with this answer: the answer itself already goes out encrypted
    client.rx = std::make_unique<crypto::Rc4>(rc4_key, 0x10);
    client.tx = std::make_unique<crypto::Rc4>(rc4_key, 0x10);
    LOG_INFO("Nova3K PSO2 server: [block {}] RC4 encryption on", client.port);
    send_packet(client, packet(0x11, 0x0C, 0x00, token), "key exchange answer");
}

void handle_packet(Client &client, const uint8_t *p, size_t size) {
    const uint8_t type = p[4], subtype = p[5];
    LOG_INFO("Nova3K PSO2 server: [{} {}] got {:02X}-{:02X} flags {:02X} ({} bytes){}: {}", role_name(client.role), client.port, type, subtype, p[6], size,
        client.rx ? " [decrypted]" : "", hex_dump(p, size));
    if (client.role != Role::Block)
        return;
    if (type == 0x11 && subtype == 0x0B && !client.rx)
        handle_key_exchange(client, p + 8, size - 8);
    else if (type == 0x11 && subtype == 0x00)
        send_packet(client, login_answer_packet(), "login OK");
    else if (type == 0x11 && subtype == 0x02)
        send_packet(client, character_list_packet(), "character list (empty)");
}

void on_data(Client &client, uint8_t *data, size_t n) {
    if (client.rx)
        client.rx->apply(data, n);
    client.in.insert(client.in.end(), data, data + n);
    size_t pos = 0;
    while (client.in.size() - pos >= 8) {
        uint32_t size = 0;
        std::memcpy(&size, client.in.data() + pos, 4);
        if (size < 8 || size > 0x1000000) {
            LOG_WARN("Nova3K PSO2 server: bad packet size {}, dropping {} bytes", size, client.in.size() - pos);
            pos = client.in.size();
            break;
        }
        if (client.in.size() - pos < size)
            break;
        handle_packet(client, client.in.data() + pos, size);
        pos += size;
    }
    client.in.erase(client.in.begin(), client.in.begin() + pos);
}

void on_connect(Client &client) {
    LOG_INFO("Nova3K PSO2 server: game connected to port {} ({})", client.port, role_name(client.role));
    switch (client.role) {
    case Role::ShipList:
        send_packet(client, ship_list_packet(), "ship list");
        client.close_after_send = true;
        break;
    case Role::BlockBalance:
        send_packet(client, block_balance_packet(), "block balance");
        client.close_after_send = true;
        break;
    case Role::Block:
        send_packet(client, hello_packet(), "hello");
        break;
    }
}

void run() {
    struct Listener {
        int fd;
        int port;
        Role role;
    };
    std::vector<Listener> listeners;
    const auto listen_on = [&](int port, Role role) {
        const int fd = socket(AF_INET, SOCK_STREAM, 0);
        if (fd < 0)
            return;
        const int yes = 1;
        setsockopt(fd, SOL_SOCKET, SO_REUSEADDR, &yes, sizeof(yes));
        sockaddr_in addr{};
        addr.sin_family = AF_INET;
        addr.sin_port = htons(uint16_t(port));
        addr.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
        if (bind(fd, reinterpret_cast<sockaddr *>(&addr), sizeof(addr)) != 0 || listen(fd, 4) != 0) {
            LOG_WARN("Nova3K PSO2 server: cannot listen on 127.0.0.1:{} ({})", port, strerror(errno));
            close(fd);
            return;
        }
        listeners.push_back({ fd, port, role });
    };
    for (int ship = 0; ship <= 9; ++ship) {
        listen_on(12094 + ship * 100, Role::ShipList);
        listen_on(12093 + ship * 100, Role::BlockBalance);
    }
    listen_on(block_port, Role::Block);
    LOG_INFO("Nova3K PSO2 server: listening on 127.0.0.1 ({} ports): ship list 12x94, block balance 12x93, block {}", listeners.size(), block_port);

    std::vector<std::unique_ptr<Client>> clients;
    std::vector<uint8_t> buffer(64 * 1024);

    while (true) {
        fd_set read_fds;
        FD_ZERO(&read_fds);
        int max_fd = 0;
        for (const auto &l : listeners) {
            FD_SET(l.fd, &read_fds);
            max_fd = std::max(max_fd, l.fd);
        }
        for (const auto &c : clients) {
            FD_SET(c->fd, &read_fds);
            max_fd = std::max(max_fd, c->fd);
        }
        timeval timeout{ 1, 0 };
        const int ready = select(max_fd + 1, &read_fds, nullptr, nullptr, &timeout);
        if (ready < 0) {
            if (errno == EINTR)
                continue;
            LOG_ERROR("Nova3K PSO2 server: select failed ({}), server stopped", strerror(errno));
            return;
        }
        const auto now = std::chrono::steady_clock::now();

        for (const auto &l : listeners) {
            if (!FD_ISSET(l.fd, &read_fds))
                continue;
            const int fd = accept(l.fd, nullptr, nullptr);
            if (fd < 0)
                continue;
            auto client = std::make_unique<Client>();
            client->fd = fd;
            client->port = l.port;
            client->role = l.role;
            client->last_data = now;
            on_connect(*client);
            if (client->close_after_send) {
                // let the game read the answer, then close (as the real query servers did)
                shutdown(fd, SHUT_WR);
            }
            clients.push_back(std::move(client));
        }

        for (auto it = clients.begin(); it != clients.end();) {
            Client &client = **it;
            bool closed = false;
            if (FD_ISSET(client.fd, &read_fds)) {
                const ssize_t n = recv(client.fd, buffer.data(), buffer.size(), 0);
                if (n <= 0) {
                    LOG_INFO("Nova3K PSO2 server: game closed connection on port {} ({}) after {} bytes", client.port, role_name(client.role), client.received);
                    closed = true;
                } else {
                    client.received += size_t(n);
                    client.last_data = now;
                    on_data(client, buffer.data(), size_t(n));
                }
            }
            const auto silent_s = std::chrono::duration_cast<std::chrono::seconds>(now - client.last_data).count();
            if (!closed && client.received == 0 && !client.reported_silence && silent_s >= 5 && !client.close_after_send) {
                LOG_INFO("Nova3K PSO2 server: game sent nothing for 5 s on port {} ({})", client.port, role_name(client.role));
                client.reported_silence = true;
            }
            if (!closed && silent_s >= 120) {
                LOG_INFO("Nova3K PSO2 server: closing idle connection on port {}", client.port);
                closed = true;
            }
            if (closed) {
                close(client.fd);
                it = clients.erase(it);
            } else {
                ++it;
            }
        }
    }
}

} // namespace

void start(const std::string &) {
    static std::atomic<bool> started{ false };
    if (started.exchange(true))
        return;
    std::thread(run).detach();
}

#endif

} // namespace nova3k::pso2::server

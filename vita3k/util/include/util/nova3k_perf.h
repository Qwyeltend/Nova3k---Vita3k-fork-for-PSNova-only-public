// Vita3K emulator project
// Copyright (C) 2026 Vita3K team
//
// This program is free software; you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation; either version 2 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License along
// with this program; if not, write to the Free Software Foundation, Inc.,
// 51 Franklin Street, Fifth Floor, Boston, MA 02110-1301 USA.

#pragma once

// Nova3K performance monitor.
//
// Once per second it writes one "Nova3K perf" line to the log with:
//  - the game's real frame rate and frame times (from sceDisplaySetFrameBuf),
//  - how many vblanks each game frame took (1 = 60 FPS pace, 2 = 30 FPS pace, ...),
//  - how regular the emulated vblank signal is (late vblanks make the game miss frames),
//  - audio buffer underruns (each one is an audible stutter),
//  - GPU-surface transfers and how long the renderer waited for them,
// and one "Nova3K threads" line with the CPU load of the busiest threads
// (100% = one full CPU core; a thread pinned near 100% is the bottleneck).
//
// Everything is header-only and cheap: a few atomics per frame and one /proc scan per second.

#include <util/log.h>

#include <algorithm>
#include <atomic>
#include <array>
#include <chrono>
#include <condition_variable>
#include <cstdint>
#include <map>
#include <mutex>
#include <string>
#include <vector>

#if defined(__linux__)
#include <cerrno>
#include <cxxabi.h>
#include <dirent.h>
#include <dlfcn.h>
#include <pthread.h>
#include <signal.h>
#include <sched.h>
#include <sys/syscall.h>
#include <ucontext.h>
#include <unistd.h>

#include <cstdlib>
#include <cstring>
#include <fstream>
#include <sstream>
#include <thread>
#endif

// 1 = write the Nova3K performance lines to the log (development builds); 0 = public builds.
#ifndef NOVA3K_PERF_MONITOR
#define NOVA3K_PERF_MONITOR 1
#endif

namespace nova3k::perf {

constexpr bool monitor_enabled = NOVA3K_PERF_MONITOR != 0;

using Clock = std::chrono::steady_clock;

inline int64_t now_us() {
    return std::chrono::duration_cast<std::chrono::microseconds>(Clock::now().time_since_epoch()).count();
}

#if defined(__linux__)
// Cores whose maximum clock is at least 80% of the fastest core ("big" cores). On the Snapdragon 8 Gen 2
// that is cpu3-7 (Cortex-A715/A710 2.8 GHz, X3 3.2 GHz); cpu0-2 are 2.0 GHz Cortex-A510 little cores,
// roughly 2-3x slower for JIT code. Empty set if the frequencies can't be read.
inline const cpu_set_t &big_cores(std::string *description = nullptr) {
    static cpu_set_t set;
    static std::string text;
    static const bool initialized = [] {
        CPU_ZERO(&set);
        std::vector<long> max_freq;
        for (int cpu = 0; cpu < 64; cpu++) {
            std::ifstream f("/sys/devices/system/cpu/cpu" + std::to_string(cpu) + "/cpufreq/cpuinfo_max_freq");
            long khz = 0;
            if (!(f >> khz))
                break;
            max_freq.push_back(khz);
        }
        const long fastest = max_freq.empty() ? 0 : *std::max_element(max_freq.begin(), max_freq.end());
        for (size_t cpu = 0; cpu < max_freq.size(); cpu++) {
            text += fmt::format("{}cpu{} {} MHz", cpu ? ", " : "", cpu, max_freq[cpu] / 1000);
            if (fastest > 0 && max_freq[cpu] * 10 >= fastest * 8) {
                CPU_SET(cpu, &set);
                text += "*";
            }
        }
        // keep everything if all cores are "big" (nothing to gain) or nothing was readable
        if (CPU_COUNT(&set) == 0 || CPU_COUNT(&set) == static_cast<int>(max_freq.size()))
            CPU_ZERO(&set);
        return true;
    }();
    (void)initialized;
    if (description)
        *description = text;
    return set;
}

// Keep the calling thread off the little cores (the Android scheduler often parks emulator threads there).
inline void pin_to_big_cores() {
    const cpu_set_t &set = big_cores();
    if (CPU_COUNT(&set) == 0)
        return;
    if (sched_setaffinity(0, sizeof(set), &set) != 0) {
        static std::atomic<bool> warned{ false };
        if (!warned.exchange(true))
            LOG_WARN("Nova3K: sched_setaffinity failed: {}", std::strerror(errno));
        return;
    }
    static std::atomic<bool> logged{ false };
    if (!logged.exchange(true)) {
        std::string text;
        big_cores(&text);
        LOG_INFO("Nova3K: emulator threads pinned to big cores (*): {}", text);
    }
}
#endif

// Give the calling host thread a readable name (shown in the thread report). Max 15 chars on Linux.
inline void name_this_thread(const std::string &name) {
#if defined(__linux__)
    // pin_to_big_cores(); -- tried in 781fbae: made things worse (threads queued for the 5 big cores 20-35% of the time)
    std::string clean;
    for (const char c : name) {
        if (clean.size() >= 15)
            break;
        // keep plain ASCII so a multi-byte character is never cut in half
        clean += (c >= 0x20 && c < 0x7F) ? c : '_';
    }
    if (!clean.empty())
        pthread_setname_np(pthread_self(), clean.c_str());
    // SDL threads (guest threads run on them) block SIGPROF; the sampling profiler needs it
    sigset_t profiler_set;
    sigemptyset(&profiler_set);
    sigaddset(&profiler_set, SIGPROF);
    pthread_sigmask(SIG_UNBLOCK, &profiler_set, nullptr);
#else
    (void)name;
#endif
}

struct Counters {
    std::atomic<uint64_t> page_faults{ 0 };
    std::atomic<uint64_t> page_fault_us{ 0 };
    std::atomic<uint64_t> audio_underruns{ 0 };
    std::atomic<uint64_t> transfers{ 0 };
    std::atomic<uint64_t> transfer_wait_us{ 0 };
    std::atomic<int64_t> last_vblank_us{ 0 };
    std::atomic<uint64_t> vblanks{ 0 };
    std::atomic<uint64_t> late_vblanks{ 0 }; // vblank more than 4 ms late
    std::atomic<int64_t> worst_vblank_gap_us{ 0 };
};

inline Counters &counters() {
    static Counters c;
    return c;
}

// Memory-protection traps handled by the emulator (texture/surface write tracking). Async-signal-safe.
inline void on_page_fault(uint64_t duration_us) {
    counters().page_faults.fetch_add(1, std::memory_order_relaxed);
    counters().page_fault_us.fetch_add(duration_us, std::memory_order_relaxed);
}

inline void on_audio_underrun() {
    counters().audio_underruns.fetch_add(1, std::memory_order_relaxed);
}

// Guest calls into the emulator (imports) that took >= 1 ms, per "thread:function", for the current second.
struct SlowCall {
    uint64_t total_us = 0;
    uint64_t count = 0;
    uint64_t max_us = 0;
};

inline std::mutex &slow_calls_mutex() {
    static std::mutex m;
    return m;
}

inline std::map<std::string, SlowCall> &slow_calls() {
    static std::map<std::string, SlowCall> calls;
    return calls;
}

inline void on_slow_import(const std::string &thread_name, const char *function, uint64_t duration_us) {
    const std::lock_guard<std::mutex> lock(slow_calls_mutex());
    SlowCall &call = slow_calls()[thread_name + ":" + (function ? function : "?")];
    call.total_us += duration_us;
    call.count++;
    call.max_us = std::max<uint64_t>(call.max_us, duration_us);
}

// Per-frame breakdown of the game's main thread: time spent inside emulator calls, by call.
struct MainFrameCalls {
    std::mutex mutex;
    std::map<std::string, SlowCall> calls;
    uint64_t total_us = 0;
};

inline MainFrameCalls &main_frame_calls() {
    static MainFrameCalls m;
    return m;
}

inline void on_main_thread_call(const std::string &function, uint64_t duration_us) {
    MainFrameCalls &m = main_frame_calls();
    const std::lock_guard<std::mutex> lock(m.mutex);
    m.total_us += duration_us;
    if (duration_us < 200)
        return; // short calls only count in the total
    SlowCall &call = m.calls[function];
    call.total_us += duration_us;
    call.count++;
    call.max_us = std::max<uint64_t>(call.max_us, duration_us);
}

inline void on_transfer_wait(uint64_t waited_us) {
    counters().transfers.fetch_add(1, std::memory_order_relaxed);
    counters().transfer_wait_us.fetch_add(waited_us, std::memory_order_relaxed);
}

// Called by the emulated vblank thread on every vblank (nominally every 16667 us).
inline void on_vblank() {
    Counters &c = counters();
    const int64_t t = now_us();
    const int64_t prev = c.last_vblank_us.exchange(t, std::memory_order_relaxed);
    c.vblanks.fetch_add(1, std::memory_order_relaxed);
    if (prev == 0)
        return;
    const int64_t gap = t - prev;
    if (gap > 16667 + 4000)
        c.late_vblanks.fetch_add(1, std::memory_order_relaxed);
    int64_t worst = c.worst_vblank_gap_us.load(std::memory_order_relaxed);
    while (gap > worst && !c.worst_vblank_gap_us.compare_exchange_weak(worst, gap, std::memory_order_relaxed)) {
    }
}

#if defined(__linux__)
struct ThreadSample {
    std::string name;
    uint64_t ticks = 0;
    uint64_t run_ns = 0; // schedstat: time on a CPU
    uint64_t queue_ns = 0; // schedstat: time runnable but waiting for a CPU
    int cpu = -1; // core it last ran on
};

// /proc/self/task/<tid>/schedstat: "<ns on cpu> <ns waiting on runqueue> <timeslices>"
inline bool read_schedstat(int tid, uint64_t &run_ns, uint64_t &queue_ns) {
    std::ifstream f("/proc/self/task/" + std::to_string(tid) + "/schedstat");
    return static_cast<bool>(f >> run_ns >> queue_ns);
}

// core the thread last ran on (field 39 of /proc/.../stat)
inline int sched_getcpu_of(int tid) {
    std::ifstream f("/proc/self/task/" + std::to_string(tid) + "/stat");
    std::string stat;
    std::getline(f, stat);
    const size_t close = stat.rfind(')');
    if (close == std::string::npos)
        return -1;
    std::istringstream fields(stat.substr(close + 2));
    std::string field;
    for (int index = 3; index <= 39 && (fields >> field); index++)
        if (index == 39)
            return std::atoi(field.c_str());
    return -1;
}

// CPU ticks (user + system) of every thread of this process.
inline std::map<int, ThreadSample> read_threads() {
    std::map<int, ThreadSample> result;
    DIR *dir = opendir("/proc/self/task");
    if (!dir)
        return result;
    while (dirent *entry = readdir(dir)) {
        if (entry->d_name[0] < '0' || entry->d_name[0] > '9')
            continue;
        const int tid = std::atoi(entry->d_name);
        const std::string base = std::string("/proc/self/task/") + entry->d_name;
        std::ifstream stat_file(base + "/stat");
        std::string stat;
        std::getline(stat_file, stat);
        const size_t close = stat.rfind(')');
        if (close == std::string::npos)
            continue;
        // fields after the command name: state(3) ppid(4) ... utime(14) stime(15)
        std::istringstream fields(stat.substr(close + 2));
        std::string field;
        uint64_t utime = 0, stime = 0;
        int cpu = -1;
        for (int index = 3; index <= 39 && (fields >> field); index++) {
            if (index == 14)
                utime = std::strtoull(field.c_str(), nullptr, 10);
            else if (index == 15)
                stime = std::strtoull(field.c_str(), nullptr, 10);
            else if (index == 39)
                cpu = std::atoi(field.c_str());
        }
        std::ifstream comm_file(base + "/comm");
        std::string name;
        std::getline(comm_file, name);
        ThreadSample sample{ name, utime + stime };
        sample.cpu = cpu;
        read_schedstat(tid, sample.run_ns, sample.queue_ns);
        result[tid] = sample;
    }
    closedir(dir);
    return result;
}
#endif

#if defined(__linux__)
// ---------------------------------------------------------------------------------------------
// Sampling profiler: a helper thread sends SIGPROF ~1000 times per second to a few target threads
// (the game's main thread and the emulator's render thread); the signal handler records the host
// program counter. Once per second the samples are grouped by function (via dladdr) or reported as
// "[jit]" when the thread was executing translated guest code.
// ---------------------------------------------------------------------------------------------
struct SampleSlot {
    std::atomic<int> tid{ 0 };
    std::atomic<uint32_t> write_index{ 0 };
    uint32_t read_index = 0;
    std::string name;
    static constexpr uint32_t capacity = 8192;
    uintptr_t pcs[capacity] = {};
    uintptr_t lrs[capacity] = {};
    uint32_t guest_pcs[capacity] = {}; // guest (Vita) address of the translated block, 0 = not in JIT code
};

// Set by the cpu module: maps a host PC inside dynarmic's code cache to the guest PC (calling thread's JIT).
inline std::atomic<uint32_t (*)(uintptr_t)> &guest_pc_resolver() {
    static std::atomic<uint32_t (*)(uintptr_t)> resolver{ nullptr };
    return resolver;
}

// Set by the cpu module: dynarmic translation statistics since the previous call.
inline std::atomic<std::string (*)()> &jit_stats_reporter() {
    static std::atomic<std::string (*)()> reporter{ nullptr };
    return reporter;
}

inline SampleSlot *sample_slots() {
    static SampleSlot slots[3];
    return slots;
}
constexpr int sample_slot_count = 3; // main thread + the two busiest Aska worker threads

inline void profiler_signal_handler(int, siginfo_t *, void *context) {
    const int saved_errno = errno;
    const int tid = static_cast<int>(syscall(SYS_gettid));
    uintptr_t pc = 0, lr = 0;
#if defined(__aarch64__)
    pc = static_cast<uintptr_t>(static_cast<ucontext_t *>(context)->uc_mcontext.pc);
    lr = static_cast<uintptr_t>(static_cast<ucontext_t *>(context)->uc_mcontext.regs[30]);
#elif defined(__x86_64__)
    pc = static_cast<uintptr_t>(static_cast<ucontext_t *>(context)->uc_mcontext.gregs[REG_RIP]);
#endif
    SampleSlot *slots = sample_slots();
    for (int i = 0; i < sample_slot_count; i++) {
        if (slots[i].tid.load(std::memory_order_relaxed) == tid) {
            const uint32_t index = slots[i].write_index.fetch_add(1, std::memory_order_relaxed);
            slots[i].pcs[index % SampleSlot::capacity] = pc;
            slots[i].lrs[index % SampleSlot::capacity] = lr;
            const auto resolver = guest_pc_resolver().load(std::memory_order_relaxed);
            slots[i].guest_pcs[index % SampleSlot::capacity] = resolver ? resolver(pc) : 0;
            break;
        }
    }
    errno = saved_errno;
}

// The sampling profiler (SIGPROF 1000x/s on three threads) found the exclusive-monitor bottleneck;
// it costs a little CPU, so it is off unless a build needs it again.
constexpr bool profiler_enabled = false;

inline std::atomic<bool> &profiler_started() {
    static std::atomic<bool> started{ false };
    return started;
}

inline void start_profiler() {
    if (profiler_started().exchange(true))
        return;
    struct sigaction action {};
    action.sa_sigaction = profiler_signal_handler;
    action.sa_flags = SA_SIGINFO | SA_RESTART;
    sigemptyset(&action.sa_mask);
    sigaction(SIGPROF, &action, nullptr);
    std::thread([] {
        name_this_thread("nova3k-prof");
        const int pid = getpid();
        while (true) {
            for (int i = 0; i < sample_slot_count; i++) {
                const int tid = sample_slots()[i].tid.load(std::memory_order_relaxed);
                if (tid > 0)
                    syscall(SYS_tgkill, pid, tid, SIGPROF);
            }
            std::this_thread::sleep_for(std::chrono::milliseconds(1));
        }
    }).detach();
}

inline std::string describe_pc(uintptr_t pc) {
    Dl_info info{};
    if (!dladdr(reinterpret_cast<void *>(pc), &info) || !info.dli_fname)
        return "[jit]"; // anonymous executable memory: dynarmic's translated guest code
    const char *module = std::strrchr(info.dli_fname, '/');
    module = module ? module + 1 : info.dli_fname;
    if (info.dli_sname) {
        int status = 0;
        char *demangled = abi::__cxa_demangle(info.dli_sname, nullptr, nullptr, &status);
        std::string name = (status == 0 && demangled) ? demangled : info.dli_sname;
        std::free(demangled);
        const size_t paren = name.find('(');
        if (paren != std::string::npos)
            name.resize(paren); // drop the argument list
        if (name.size() > 70)
            name = "..." + name.substr(name.size() - 67);
        return std::string(module) + ":" + name;
    }
    return fmt::format("{}+0x{:X}", module, pc - reinterpret_cast<uintptr_t>(info.dli_fbase));
}

// Pick the targets by thread name, then report what each target was doing during the last second.
inline void profiler_report(const std::map<int, ThreadSample> &threads, const std::map<int, ThreadSample> &previous) {
    start_profiler();
    SampleSlot *slots = sample_slots();
    if (const auto reporter = jit_stats_reporter().load()) {
        const std::string stats = reporter();
        if (!stats.empty())
            LOG_INFO("Nova3K jit: {}", stats);
    }
    // slot 0: the busiest thread that is not an emulator helper (normally the game's main thread)
    for (int i = 0; i < sample_slot_count; i++) {
        SampleSlot &slot = slots[i];
        const uint32_t end = slot.write_index.load(std::memory_order_relaxed);
        const uint32_t begin = std::max<uint32_t>(slot.read_index, end > SampleSlot::capacity ? end - SampleSlot::capacity : 0);
        if (slot.tid.load() > 0 && end > begin) {
            std::map<std::string, uint32_t> hits;
            for (uint32_t k = begin; k < end; k++) {
                std::string where = describe_pc(slot.pcs[k % SampleSlot::capacity]);
                // a leaf libc wrapper (syscall, futex...) says nothing by itself: name its caller
                if (where.starts_with("libc.so:"))
                    where += "<-" + describe_pc(slot.lrs[k % SampleSlot::capacity]);
                hits[where]++;
            }
            std::vector<std::pair<uint32_t, std::string>> sorted;
            for (auto &[name, count] : hits)
                sorted.emplace_back(count, name);
            std::sort(sorted.rbegin(), sorted.rend());
            std::string text;
            const uint32_t total = end - begin;
            for (size_t k = 0; k < sorted.size() && k < 12; k++)
                text += fmt::format("{}{} {:.0f}%", k ? ", " : "", sorted[k].second, sorted[k].first * 100.0 / total);
            LOG_INFO("Nova3K profile {} ({} samples): {}", slot.name, total, text);

            // which guest code the translated-code samples belong to (translated block start addresses)
            std::map<uint32_t, uint32_t> guest_hits;
            std::map<std::string, uint32_t> other_jit; // [jit] samples that are not inside a translated block
            uint32_t guest_total = 0;
            for (uint32_t k = begin; k < end; k++) {
                const uint32_t guest_pc = slot.guest_pcs[k % SampleSlot::capacity];
                if (guest_pc >= 0xF0000000)
                    other_jit[fmt::format("prelude+{:X}", (guest_pc & 0x0FFFFFFF) & ~0xFu)]++;
                else if (guest_pc == 0xE0000000)
                    other_jit[describe_pc(slot.pcs[k % SampleSlot::capacity]) == "[jit]" ? "[jit] without guest cpu" : "-"]++;
                else if (guest_pc == 0xE0000001)
                    other_jit[describe_pc(slot.pcs[k % SampleSlot::capacity]) == "[jit]" ? "[jit] outside code cache" : "-"]++;
                else if (guest_pc) {
                    guest_hits[guest_pc]++;
                    guest_total++;
                }
            }
            other_jit.erase("-");
            if (!other_jit.empty()) {
                std::vector<std::pair<uint32_t, std::string>> other_sorted;
                for (auto &[name, count] : other_jit)
                    other_sorted.emplace_back(count, name);
                std::sort(other_sorted.rbegin(), other_sorted.rend());
                std::string other_text;
                for (size_t k = 0; k < other_sorted.size() && k < 12; k++)
                    other_text += fmt::format("{}{} {:.1f}%", k ? ", " : "", other_sorted[k].second, other_sorted[k].first * 100.0 / total);
                LOG_INFO("Nova3K jit-other {}: {}", slot.name, other_text);
            }
            if (guest_total) {
                std::vector<std::pair<uint32_t, uint32_t>> guest_sorted;
                for (auto &[guest_pc, count] : guest_hits)
                    guest_sorted.emplace_back(count, guest_pc);
                std::sort(guest_sorted.rbegin(), guest_sorted.rend());
                std::string guest_text;
                for (size_t k = 0; k < guest_sorted.size() && k < 20; k++)
                    guest_text += fmt::format("{}{:08X} {:.1f}%", k ? " " : "", guest_sorted[k].second, guest_sorted[k].first * 100.0 / total);
                LOG_INFO("Nova3K guest {} ({:.0f}% of samples in game code, {} blocks): {}", slot.name, guest_total * 100.0 / total,
                    guest_hits.size(), guest_text);
            }
        }
        slot.read_index = end;
    }
    // (re)select targets
    int best_tid = 0;
    for (const auto &[tid, sample] : threads) {
        if (sample.name == "PCSG00351" || sample.name == "PCSH00143") {
            best_tid = tid;
            break;
        }
    }
    if (best_tid && slots[0].tid.load() != best_tid) {
        slots[0].name = "main";
        slots[0].tid.store(best_tid);
    }
    // slots 1-2: the two Aska worker threads that used the most CPU in the last second
    std::vector<std::pair<uint64_t, int>> workers;
    for (const auto &[tid, sample] : threads) {
        if (sample.name != "Aska")
            continue;
        const auto it = previous.find(tid);
        if (it != previous.end() && sample.ticks >= it->second.ticks)
            workers.emplace_back(sample.ticks - it->second.ticks, tid);
    }
    std::sort(workers.rbegin(), workers.rend());
    for (int i = 1; i < sample_slot_count; i++) {
        if (static_cast<size_t>(i - 1) >= workers.size())
            break;
        const int tid = workers[i - 1].second;
        if (slots[i].tid.load() != tid) {
            slots[i].name = "Aska#" + std::to_string(tid);
            slots[i].tid.store(tid);
        }
    }
}
#endif

// Once-per-second report. Runs on its own thread ("nova3k-report"): reading /proc, symbolizing profiler
// samples and writing the log took ~45 ms, and done on the frame-flip thread it stalled one frame per second.
inline void report_window(const std::vector<int64_t> &frame_times_us, const std::array<uint64_t, 5> &hist, int64_t elapsed_us, int64_t t) {
#if defined(__linux__)
    static std::map<int, ThreadSample> previous_threads;
    static int64_t previous_threads_us = 0;
#endif
    // ---- frame report ----
    std::vector<int64_t> sorted = frame_times_us;
    std::sort(sorted.begin(), sorted.end());
    int64_t sum = 0;
    for (const int64_t v : sorted)
        sum += v;
    const double fps = frame_times_us.size() * 1e6 / static_cast<double>(elapsed_us);
    const double avg_ms = sum / 1000.0 / sorted.size();
    const double p95_ms = sorted[std::min(sorted.size() - 1, sorted.size() * 95 / 100)] / 1000.0;
    const double max_ms = sorted.back() / 1000.0;

    Counters &c = counters();
    const uint64_t underruns = c.audio_underruns.exchange(0, std::memory_order_relaxed);
    const uint64_t transfers = c.transfers.exchange(0, std::memory_order_relaxed);
    const uint64_t transfer_wait_us = c.transfer_wait_us.exchange(0, std::memory_order_relaxed);
    const uint64_t page_faults = c.page_faults.exchange(0, std::memory_order_relaxed);
    const uint64_t page_fault_us = c.page_fault_us.exchange(0, std::memory_order_relaxed);
    const uint64_t vblanks = c.vblanks.exchange(0, std::memory_order_relaxed);
    const uint64_t late_vblanks = c.late_vblanks.exchange(0, std::memory_order_relaxed);
    const int64_t worst_vblank_gap_us = c.worst_vblank_gap_us.exchange(0, std::memory_order_relaxed);

    LOG_INFO("Nova3K perf: {:.1f} fps | frame avg {:.1f} ms, p95 {:.1f} ms, max {:.1f} ms | vblanks/frame 1:{} 2:{} 3:{} 4:{} 5+:{} | "
             "vblank signal {}/s, late {}, worst gap {:.1f} ms | audio underruns {} | gpu-surface transfers {}, waited {:.1f} ms | "
             "memory traps {}, {:.1f} ms",
        fps, avg_ms, p95_ms, max_ms, hist[0], hist[1], hist[2], hist[3], hist[4],
        vblanks, late_vblanks, worst_vblank_gap_us / 1000.0, underruns, transfers, transfer_wait_us / 1000.0,
        page_faults, page_fault_us / 1000.0);

    // ---- where guest threads blocked inside emulator calls ----
    {
        std::vector<std::pair<SlowCall, std::string>> calls;
        {
            const std::lock_guard<std::mutex> calls_lock(slow_calls_mutex());
            for (auto &[key, call] : slow_calls())
                calls.emplace_back(call, key);
            slow_calls().clear();
        }
        std::sort(calls.begin(), calls.end(), [](const auto &a, const auto &b) { return a.first.total_us > b.first.total_us; });
        std::string text;
        for (size_t i = 0; i < calls.size() && i < 10; i++)
            text += fmt::format("{}{} {:.0f} ms ({}x, max {:.0f})", i ? ", " : "", calls[i].second,
                calls[i].first.total_us / 1000.0, calls[i].first.count, calls[i].first.max_us / 1000.0);
        if (!text.empty())
            LOG_INFO("Nova3K waits: {}", text);
    }

    // ---- thread report ----
#if defined(__linux__)
    std::map<int, ThreadSample> threads = read_threads();
    if (!previous_threads.empty() && previous_threads_us != 0) {
        static const long ticks_per_second = sysconf(_SC_CLK_TCK);
        const double seconds = (t - previous_threads_us) / 1e6;
        std::vector<std::pair<double, std::string>> loads;
        double total = 0;
        for (const auto &[tid, sample] : threads) {
            const auto it = previous_threads.find(tid);
            if (it == previous_threads.end() || sample.ticks < it->second.ticks)
                continue;
            const double percent = (sample.ticks - it->second.ticks) * 100.0 / ticks_per_second / seconds;
            total += percent;
            const double queue_percent = sample.queue_ns >= it->second.queue_ns ? (sample.queue_ns - it->second.queue_ns) / 1e7 / seconds : 0.0;
            if (percent >= 3.0)
                loads.emplace_back(percent, fmt::format("{}#{} (wait core {:.0f}%, cpu{})", sample.name, tid, queue_percent, sample.cpu));
        }
        std::sort(loads.rbegin(), loads.rend());
        std::string text;
        for (size_t i = 0; i < loads.size() && i < 10; i++)
            text += fmt::format("{}{} {:.0f}%", i ? ", " : "", loads[i].second, loads[i].first);
        LOG_INFO("Nova3K threads: total {:.0f}% | {} [wait core = runnable but no free CPU core]", total, text);
    }
    if (profiler_enabled)
        profiler_report(threads, previous_threads);
    else if (const auto reporter = jit_stats_reporter().load()) {
        const std::string stats = reporter();
        if (!stats.empty())
            LOG_INFO("Nova3K jit: {}", stats);
    }
    previous_threads = std::move(threads);
    previous_threads_us = t;
#endif

}

struct ReportQueue {
    std::mutex mutex;
    std::condition_variable cv;
    bool pending = false;
    std::vector<int64_t> frame_times_us;
    std::array<uint64_t, 5> hist{};
    int64_t elapsed_us = 0;
    int64_t t = 0;
};

inline ReportQueue &report_queue() {
    static ReportQueue q;
    return q;
}

inline void post_report(std::vector<int64_t> frame_times_us, const uint64_t (&hist)[5], int64_t elapsed_us, int64_t t) {
    ReportQueue &q = report_queue();
    static std::once_flag started;
    std::call_once(started, [] {
        std::thread([] {
            name_this_thread("nova3k-report");
            ReportQueue &queue = report_queue();
            while (true) {
                std::vector<int64_t> times;
                std::array<uint64_t, 5> h{};
                int64_t elapsed = 0, when = 0;
                {
                    std::unique_lock<std::mutex> lock(queue.mutex);
                    queue.cv.wait(lock, [&] { return queue.pending; });
                    queue.pending = false;
                    times.swap(queue.frame_times_us);
                    h = queue.hist;
                    elapsed = queue.elapsed_us;
                    when = queue.t;
                }
                report_window(times, h, elapsed, when);
            }
        }).detach();
    });
    {
        const std::lock_guard<std::mutex> lock(q.mutex);
        q.frame_times_us = std::move(frame_times_us);
        std::copy(std::begin(hist), std::end(hist), q.hist.begin());
        q.elapsed_us = elapsed_us;
        q.t = t;
        q.pending = true;
    }
    q.cv.notify_one();
}

// Called on every game frame flip (sceDisplaySetFrameBuf with NEXTFRAME).
inline void on_game_frame(uint64_t vblank_count) {
    if constexpr (!monitor_enabled)
        return;
    static std::mutex mutex;
    static int64_t window_start_us = 0;
    static int64_t last_frame_us = 0;
    static uint64_t last_vblank_count = 0;
    static std::vector<int64_t> frame_times_us;
    static uint64_t vblank_hist[5] = {}; // 1, 2, 3, 4, 5+ vblanks per frame

    const std::lock_guard<std::mutex> lock(mutex);
    const int64_t t = now_us();

    // slow frame breakdown: what the main thread did since the previous flip
    {
        MainFrameCalls &m = main_frame_calls();
        std::lock_guard<std::mutex> calls_lock(m.mutex);
        const int64_t frame_us = last_frame_us ? t - last_frame_us : 0;
        static int logged_this_second = 0;
        // main thread scheduling during this frame: on-CPU time vs runnable-but-waiting-for-a-CPU time
        std::string sched_text;
#if defined(__linux__)
        {
            static int sched_tid = 0;
            static uint64_t prev_run_ns = 0, prev_queue_ns = 0;
            const int tid = sample_slots()[0].tid.load(std::memory_order_relaxed);
            uint64_t run_ns = 0, queue_ns = 0;
            if (tid > 0 && read_schedstat(tid, run_ns, queue_ns)) {
                if (tid == sched_tid && run_ns >= prev_run_ns && queue_ns >= prev_queue_ns)
                    sched_text = fmt::format(" | on CPU {:.1f} ms, waiting for a free core {:.1f} ms, cpu{}", (run_ns - prev_run_ns) / 1e6,
                        (queue_ns - prev_queue_ns) / 1e6, sched_getcpu_of(tid));
                sched_tid = tid;
                prev_run_ns = run_ns;
                prev_queue_ns = queue_ns;
            }
        }
#endif
        if (frame_us >= 50000 && logged_this_second < 5) {
            logged_this_second++;
            std::vector<std::pair<SlowCall, std::string>> calls;
            for (auto &[name, call] : m.calls)
                calls.emplace_back(call, name);
            std::sort(calls.begin(), calls.end(), [](const auto &a, const auto &b) { return a.first.total_us > b.first.total_us; });
            std::string text;
            for (size_t i = 0; i < calls.size() && i < 6; i++)
                text += fmt::format("{}{} {:.1f} ms ({}x, max {:.1f})", i ? ", " : "", calls[i].second,
                    calls[i].first.total_us / 1000.0, calls[i].first.count, calls[i].first.max_us / 1000.0);
            LOG_INFO("Nova3K slow frame {:.1f} ms: main thread outside emulator calls {:.1f} ms, inside {:.1f} ms{} | {}",
                frame_us / 1000.0, std::max<int64_t>(0, frame_us - static_cast<int64_t>(m.total_us)) / 1000.0, m.total_us / 1000.0,
                sched_text, text);
        }
        if (t - window_start_us >= 1000000)
            logged_this_second = 0;
        m.calls.clear();
        m.total_us = 0;
    }

    if (last_frame_us != 0) {
        frame_times_us.push_back(t - last_frame_us);
        const uint64_t vblanks = vblank_count - last_vblank_count;
        if (vblanks >= 1)
            vblank_hist[std::min<uint64_t>(vblanks, 5) - 1]++;
    } else {
        window_start_us = t;
    }
    last_frame_us = t;
    last_vblank_count = vblank_count;

    const int64_t elapsed_us = t - window_start_us;
    if (elapsed_us < 1000000 || frame_times_us.empty())
        return;

    post_report(frame_times_us, vblank_hist, elapsed_us, t);

    frame_times_us = {};
    std::fill(std::begin(vblank_hist), std::end(vblank_hist), 0);
    window_start_us = t;
}

} // namespace nova3k::perf

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

#include <util/log.h>
#include <util/nova3k_game.h>

#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#include <Windows.h>
#endif

#include <spdlog/async.h>
#include <spdlog/sinks/basic_file_sink.h>
#include <spdlog/sinks/dup_filter_sink.h>
#include <spdlog/sinks/msvc_sink.h>
#ifdef __ANDROID__
#include <spdlog/sinks/android_sink.h>
#else
#include <spdlog/sinks/stdout_color_sinks.h>
#endif

#include <functional>
#if defined(__linux__)
#include <atomic>
#include <csignal>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <dlfcn.h>
#include <fcntl.h>
#include <sys/syscall.h>
#include <ucontext.h>
#include <unistd.h>
#include <unwind.h>
#endif
#include <iostream>
#include <mutex>
#include <vector>

namespace logging {

static const fs::path &LOG_FILE_NAME = "vita3k.log";
static const char *LOG_PATTERN = "%^[%H:%M:%S.%e] |%L| [%!]: %v%$";
static constexpr size_t ASYNC_LOG_QUEUE_SIZE = 65536;
static std::vector<spdlog::sink_ptr> sinks;
static std::once_flag s_async_logging_once;

static std::function<void(std::string, int)> s_log_callback;
static std::mutex s_log_callback_mutex;

static void register_log_exception_handler();
#if defined(__linux__) && !defined(_WIN32)
static void set_crash_file(const fs::path &path);
#endif
static void rebuild_default_logger();

static void flush() {
    spdlog::details::registry::instance().flush_all();
}

template <typename Mutex>
class callback_sink final : public spdlog::sinks::base_sink<Mutex> {
protected:
    void sink_it_(const spdlog::details::log_msg &msg) override {
        std::function<void(std::string, int)> callback;
        {
            const std::lock_guard<std::mutex> lock(s_log_callback_mutex);
            callback = s_log_callback;
        }

        if (!callback)
            return;

        spdlog::memory_buf_t formatted;
        spdlog::sinks::base_sink<Mutex>::formatter_->format(msg, formatted);
        callback(fmt::to_string(formatted), static_cast<int>(msg.level));
    }

    void flush_() override {};
};

using callback_sink_mt = callback_sink<std::mutex>;

void set_log_callback(std::function<void(std::string, int)> cb) {
    const std::lock_guard<std::mutex> lock(s_log_callback_mutex);
    s_log_callback = std::move(cb);
}

ExitCode init(const Root &root_paths, bool use_stdout) {
    sinks.clear();
    if (use_stdout)
#ifdef __ANDROID__
        sinks.push_back(std::make_shared<spdlog::sinks::android_sink_mt>("Vita3K"));
#else
        sinks.push_back(std::make_shared<spdlog::sinks::stdout_color_sink_mt>());
#endif

#ifndef __ANDROID__
    sinks.push_back(std::make_shared<callback_sink_mt>());
#endif

    // Nova3K: keep the previous session's log. After a crash Android restarts the app,
    // and the new session would otherwise overwrite the log that shows the crash.
    {
        const fs::path log_file = root_paths.get_log_path() / LOG_FILE_NAME;
        boost::system::error_code ec;
        if (fs::exists(log_file, ec))
            fs::rename(log_file, root_paths.get_log_path() / "vita3k.prev.log", ec);
    }

    if (add_sink(root_paths.get_log_path() / LOG_FILE_NAME) != Success)
        return InitConfigFailed;

    spdlog::set_error_handler([](const std::string &msg) {
        std::cerr << "spdlog error: " << msg << std::endl;
        assert(0);
    });

#ifdef _WIN32
    // set console codepage to UTF-8
    SetConsoleOutputCP(65001);
    SetConsoleTitle("Vita3K PSVita Emulator");
#endif

#ifdef __ANDROID__
    // needed, otherwise the log file contains nothing
    spdlog::flush_on(spdlog::level::trace);
#endif

#if defined(__linux__) && !defined(_WIN32)
    set_crash_file(root_paths.get_log_path() / "nova3k_crash.txt");
#endif
    register_log_exception_handler();

    static std::terminate_handler old_terminate = nullptr;
    old_terminate = std::set_terminate([]() {
        try {
            auto eptr = std::current_exception();
            if (eptr) {
                std::rethrow_exception(eptr);
            } else {
                LOG_CRITICAL("Unhandled 'std::terminate()' call");
            }
        } catch (const std::exception &e) {
            LOG_CRITICAL("Unhandled C++ exception. {}", e.what());
        } catch (...) {
            LOG_CRITICAL("Unhandled C++ exception. UNKNOWN");
        }
        flush();
        if (old_terminate)
            old_terminate();
    });
    return Success;
}

void set_level(spdlog::level::level_enum log_level) {
    spdlog::set_level(log_level);
}

ExitCode add_sink(const fs::path &log_path) {
    try {
        sinks.push_back(std::make_shared<spdlog::sinks::basic_file_sink_mt>(log_path.generic_path().native(), true));
    } catch (const spdlog::spdlog_ex &ex) {
        std::cerr << "File log initialization failed: " << ex.what() << std::endl;
        return InitConfigFailed;
    }

#ifdef _MSC_VER
    sinks.push_back(std::make_shared<spdlog::sinks::msvc_sink_mt>());
#endif

    rebuild_default_logger();
    return Success;
}

void rebuild_default_logger() {
    std::call_once(s_async_logging_once, []() {
        spdlog::init_thread_pool(ASYNC_LOG_QUEUE_SIZE, 1);
    });

    auto duplicate_filter = std::make_shared<spdlog::sinks::dup_filter_sink_mt>(std::chrono::seconds(2));
    for (const auto &sink : sinks)
        duplicate_filter->add_sink(sink);

    auto logger = std::make_shared<spdlog::async_logger>(
        "vita3k logger",
        duplicate_filter,
        spdlog::thread_pool(),
        spdlog::async_overflow_policy::overrun_oldest);
    spdlog::set_default_logger(std::move(logger));
    spdlog::set_pattern(LOG_PATTERN);
}

// log exceptions and flush log file on exceptions
#ifdef _WIN32
static LONG WINAPI exception_handler(PEXCEPTION_POINTERS pExp) noexcept {
    const unsigned ec = pExp->ExceptionRecord->ExceptionCode;
    switch (ec) {
    case EXCEPTION_ACCESS_VIOLATION:
        LOG_CRITICAL("Exception EXCEPTION_ACCESS_VIOLATION ({}). ", log_hex(ec));
        switch (pExp->ExceptionRecord->ExceptionInformation[0]) {
        case 0:
            LOG_CRITICAL("Read violation at address {}.", log_hex(pExp->ExceptionRecord->ExceptionInformation[1]));
            break;
        case 1:
            LOG_CRITICAL("Write violation at address {}.", log_hex(pExp->ExceptionRecord->ExceptionInformation[1]));
            break;
        case 8:
            LOG_CRITICAL("DEP violation at address {}.", log_hex(pExp->ExceptionRecord->ExceptionInformation[1]));
            break;
        default:
            break;
        }
        break;
    case EXCEPTION_ARRAY_BOUNDS_EXCEEDED:
        LOG_CRITICAL("Exception EXCEPTION_ARRAY_BOUNDS_EXCEEDED ({}). ", log_hex(ec));
        break;
    case EXCEPTION_DATATYPE_MISALIGNMENT:
        LOG_CRITICAL("Exception EXCEPTION_DATATYPE_MISALIGNMENT ({}). ", log_hex(ec));
        break;
    case EXCEPTION_FLT_DIVIDE_BY_ZERO:
        LOG_CRITICAL("Exception EXCEPTION_FLT_DIVIDE_BY_ZERO ({}). ", log_hex(ec));
        break;
    case EXCEPTION_ILLEGAL_INSTRUCTION:
        LOG_CRITICAL("Exception EXCEPTION_ILLEGAL_INSTRUCTION ({}). ", log_hex(ec));
        break;
    case EXCEPTION_IN_PAGE_ERROR:
        LOG_CRITICAL("Exception EXCEPTION_IN_PAGE_ERROR ({}). ", log_hex(ec));
        break;
    case EXCEPTION_INT_DIVIDE_BY_ZERO:
        LOG_CRITICAL("Exception EXCEPTION_INT_DIVIDE_BY_ZERO ({}). ", log_hex(ec));
        break;
    case EXCEPTION_PRIV_INSTRUCTION:
        LOG_CRITICAL("Exception EXCEPTION_PRIV_INSTRUCTION ({}). ", log_hex(ec));
        break;
    case EXCEPTION_STACK_OVERFLOW:
        LOG_CRITICAL("Exception EXCEPTION_STACK_OVERFLOW ({}). ", log_hex(ec));
        break;
    default:
        return EXCEPTION_CONTINUE_SEARCH;
    }
    flush();
    return EXCEPTION_CONTINUE_SEARCH;
}

void register_log_exception_handler() {
    if (!AddVectoredExceptionHandler(0, exception_handler)) {
        LOG_CRITICAL("Failed to register an exception handler");
    }
}

#elif defined(__linux__)
// Nova3K: native crash reporter for Linux/Android. Writes the signal, the crashing address and a
// backtrace (with library offsets, symbolizable with libVita3K-symbols.zip) to nova3k_crash.txt
// next to the log, flushes the log, then lets the previous handler / default action run.
static char crash_file_path[1024] = {};
static struct sigaction previous_actions[NSIG];

static _Unwind_Reason_Code crash_unwind(struct _Unwind_Context *ctx, void *arg) {
    auto *state = static_cast<std::pair<int, int> *>(arg); // fd, frame count
    const uintptr_t pc = _Unwind_GetIP(ctx);
    if (!pc || state->second >= 48)
        return _URC_END_OF_STACK;
    Dl_info info{};
    char line[512];
    if (dladdr(reinterpret_cast<void *>(pc), &info) && info.dli_fname) {
        const char *module = std::strrchr(info.dli_fname, '/');
        module = module ? module + 1 : info.dli_fname;
        std::snprintf(line, sizeof(line), "#%02d %s+0x%zx %s\n", state->second, module,
            static_cast<size_t>(pc - reinterpret_cast<uintptr_t>(info.dli_fbase)), info.dli_sname ? info.dli_sname : "");
    } else {
        std::snprintf(line, sizeof(line), "#%02d 0x%zx\n", state->second, static_cast<size_t>(pc));
    }
    (void)!write(state->first, line, std::strlen(line));
    state->second++;
    return _URC_NO_REASON;
}

static std::atomic<int> crash_reports_written{ 0 };

// Nova3K: writes one crash report to nova3k_crash.txt. The first report of a run replaces the old file,
// later ones (another thread crashing at the same time, or the abort that follows) are appended.
void write_crash_report(int sig, void *info_ptr, void *uctx) {
    siginfo_t *info = static_cast<siginfo_t *>(info_ptr);
    const bool first = crash_reports_written.fetch_add(1) == 0;
    const int fd = open(crash_file_path, O_WRONLY | O_CREAT | (first ? O_TRUNC : O_APPEND), 0644);
    if (fd >= 0) {
        if (!first)
            (void)!write(fd, "\n", 1);
        char line[512];
        uintptr_t pc = 0;
#if defined(__aarch64__)
        pc = static_cast<uintptr_t>(static_cast<ucontext_t *>(uctx)->uc_mcontext.pc);
#endif
        Dl_info dl{};
        size_t pc_off = 0;
        const char *pc_mod = "?";
        if (pc && dladdr(reinterpret_cast<void *>(pc), &dl) && dl.dli_fname) {
            pc_off = pc - reinterpret_cast<uintptr_t>(dl.dli_fbase);
            pc_mod = std::strrchr(dl.dli_fname, '/') ? std::strrchr(dl.dli_fname, '/') + 1 : dl.dli_fname;
        }
        std::snprintf(line, sizeof(line), "Nova3K crash: signal %d (code %d), fault address %p, pc %s+0x%zx, thread %ld\nbacktrace:\n",
            sig, info ? info->si_code : 0, info ? info->si_addr : nullptr, pc_mod, pc_off, static_cast<long>(syscall(SYS_gettid)));
        (void)!write(fd, line, std::strlen(line));
        std::pair<int, int> state{ fd, 0 };
        _Unwind_Backtrace(crash_unwind, &state);
        const auto &rt = nova3k::game::render_trace();
        if (rt.commands) {
            std::snprintf(line, sizeof(line), "render thread: command #%llu, opcode %u, flags 0x%x, command %p, context %p, stage %s\n",
                static_cast<unsigned long long>(rt.commands), rt.opcode, rt.flags, reinterpret_cast<void *>(rt.command),
                reinterpret_cast<void *>(rt.context), rt.stage == 1 ? "run" : rt.stage == 2 ? "free" : "-");
            (void)!write(fd, line, std::strlen(line));
        }
        close(fd);
    }
}

static void crash_handler(int sig, siginfo_t *info, void *uctx) {
    write_crash_report(sig, info, uctx);
    LOG_CRITICAL("Nova3K: fatal signal {} - details in nova3k_crash.txt", sig);
    flush();
    usleep(300000);
    // chain to whoever was there before (Android's debuggerd writes a tombstone), else default action
    const struct sigaction &prev = previous_actions[sig];
    if (prev.sa_flags & SA_SIGINFO) {
        if (prev.sa_sigaction) {
            prev.sa_sigaction(sig, info, uctx);
            return;
        }
    } else if (prev.sa_handler != SIG_DFL && prev.sa_handler != SIG_IGN) {
        prev.sa_handler(sig);
        return;
    }
    signal(sig, SIG_DFL);
    raise(sig);
}

static void set_crash_file(const fs::path &path) {
    std::snprintf(crash_file_path, sizeof(crash_file_path), "%s", path.string().c_str());
}

void register_log_exception_handler() {
    // while a game runs, the emulator's memory-protection handler replaces this one for SIGSEGV
    for (const int sig : { SIGABRT, SIGBUS, SIGILL, SIGFPE, SIGSEGV }) {
        struct sigaction action {};
        action.sa_sigaction = crash_handler;
        action.sa_flags = SA_SIGINFO | SA_ONSTACK;
        sigemptyset(&action.sa_mask);
        sigaction(sig, &action, &previous_actions[sig]);
    }
}
#else
void register_log_exception_handler() {}
#endif
#if !defined(__linux__) || defined(_WIN32)
void write_crash_report(int, void *, void *) {}
#endif
} // namespace logging

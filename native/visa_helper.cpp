// SPDX-License-Identifier: MIT
//
// Optional official-VISA transport.  This helper intentionally contains no
// vendor header, import library, or DLL: a 64-bit VISA runtime already present
// in Windows is loaded from System32 at run time.
#include "stdio_mode.hpp"

#ifdef _WIN32
#include <windows.h>
#endif

#include <algorithm>
#include <cctype>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <iostream>
#include <sstream>
#include <stdexcept>
#include <string>
#include <vector>

#ifndef _WIN32
int main() { std::cerr << "visa_helper is only available on Windows.\n"; return 1; }
#else

// Definitions are from the public IVI VISA specification (visa.h).  Keeping
// the small ABI surface here lets the portable remain independent of a
// particular NI/Keysight installation and avoids redistributing their SDK.
using ViStatus = std::int32_t;
using ViSession = std::uint32_t;
using ViFindList = std::uint32_t;
using ViUInt32 = std::uint32_t;
using ViAccessMode = std::uint32_t;
using ViAttr = std::uint32_t;
using ViAttrState = uintptr_t;
using ViBuf = unsigned char *;
using ViPBuf = unsigned char *;

constexpr ViStatus VI_SUCCESS = 0;
constexpr ViSession VI_NULL = 0;
constexpr ViAccessMode VI_EXCLUSIVE_LOCK = 1;
constexpr ViAttr VI_ATTR_TMO_VALUE = 0x3FFF001A;
constexpr ViAttr VI_ATTR_SEND_END_EN = 0x3FFF0016;
constexpr ViAttr VI_ATTR_TERMCHAR_EN = 0x3FFF0038;
constexpr ViUInt32 VI_FIND_BUFLEN = 256;
constexpr ViStatus VI_SUCCESS_MAX_CNT = 0x3FFF0006;
constexpr ViStatus VI_ERROR_RSRC_NFOUND = static_cast<ViStatus>(0xBFFF0011L);
constexpr std::uint16_t VI_GPIB_REN_DEASSERT = 0;
constexpr std::uint16_t VI_GPIB_REN_DEASSERT_GTL = 1;
constexpr std::uint16_t VI_GPIB_REN_ADDRESS_GTL = 6;
constexpr std::size_t MAX_PAYLOAD = 16 * 1024;
constexpr std::size_t MAX_LINE = 64 * 1024;

using ViOpenDefaultRM = ViStatus (__stdcall *)(ViSession *);
using ViFindRsrc = ViStatus (__stdcall *)(ViSession, const char *, ViFindList *, ViUInt32 *, char *);
using ViFindNext = ViStatus (__stdcall *)(ViFindList, char *);
using ViOpen = ViStatus (__stdcall *)(ViSession, const char *, ViAccessMode, ViUInt32, ViSession *);
using ViClose = ViStatus (__stdcall *)(ViSession);
using ViSetAttribute = ViStatus (__stdcall *)(ViSession, ViAttr, ViAttrState);
using ViWrite = ViStatus (__stdcall *)(ViSession, ViBuf, ViUInt32, ViUInt32 *);
using ViRead = ViStatus (__stdcall *)(ViSession, ViPBuf, ViUInt32, ViUInt32 *);
using ViClear = ViStatus (__stdcall *)(ViSession);
using ViGpibControlREN = ViStatus (__stdcall *)(ViSession, std::uint16_t);

static std::string hex(const std::string &value) {
    static const char digits[] = "0123456789abcdef";
    std::string out; out.reserve(value.size() * 2);
    for (unsigned char byte : value) { out += digits[byte >> 4]; out += digits[byte & 15]; }
    return out;
}
static std::string unhex(const std::string &value) {
    if (value == "-") return "";
    if ((value.size() & 1U) || value.size() > MAX_PAYLOAD * 2)
        throw std::runtime_error("Invalid VISA payload size");
    auto nibble = [](char c) -> unsigned char {
        if (c >= '0' && c <= '9') return static_cast<unsigned char>(c - '0');
        if (c >= 'a' && c <= 'f') return static_cast<unsigned char>(c - 'a' + 10);
        throw std::runtime_error("Invalid VISA hex payload");
    };
    std::string out; out.reserve(value.size() / 2);
    for (std::size_t i = 0; i < value.size(); i += 2)
        out += static_cast<char>((nibble(value[i]) << 4) | nibble(value[i + 1]));
    return out;
}
static void reply(const char *status, const std::string &payload = "") {
    std::cout << "KIV\t" << status << '\t' << hex(payload) << std::endl;
}
static bool succeeded(ViStatus status) { return status >= VI_SUCCESS; }
static void check(ViStatus status, const char *operation) {
    if (!succeeded(status)) {
        std::string help;
        if (static_cast<ViUInt32>(status) == 0xBFFF000F) help = " The instrument is busy or exclusively locked. Close other instrument software and retry.";
        else if (static_cast<ViUInt32>(status) == 0xBFFF0015) help = " The instrument timed out. Check its power, cable, address and interface settings before reconnecting.";
        else if (static_cast<ViUInt32>(status) == 0xBFFF0011) help = " The resource was not found by official VISA. Check the device driver and 64-bit VISA configuration, then click Refresh.";
        throw std::runtime_error(std::string("VISA ") + operation + " failed (" + std::to_string(status) + ")." + help);
    }
}
static bool read_bounded_line(std::string &line) {
    line.clear();
    char character = 0;
    while (std::cin.get(character)) {
        if (character == '\n') return true;
        if (character == '\r') continue;
        if (line.size() == MAX_LINE) {
            while (std::cin.get(character) && character != '\n') {}
            throw std::runtime_error("VISA helper request exceeds 64 KiB.");
        }
        line += character;
    }
    return !line.empty();
}

class Visa {
    HMODULE library_ = nullptr;
public:
    ViOpenDefaultRM open_default_rm = nullptr;
    ViFindRsrc find_rsrc = nullptr;
    ViFindNext find_next = nullptr;
    ViOpen open = nullptr;
    ViClose close = nullptr;
    ViSetAttribute set_attribute = nullptr;
    ViWrite write = nullptr;
    ViRead read = nullptr;
    ViClear clear = nullptr;
    ViGpibControlREN gpib_control_ren = nullptr;

    Visa() {
#ifdef KOHDALAB_VISA_TEST_DLL
        // Test-only compile-time override. Production builds never consult an
        // environment variable, the application directory, PATH, or CWD.
        library_ = LoadLibraryExW(KOHDALAB_VISA_TEST_DLL, nullptr, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR);
#else
        wchar_t system[MAX_PATH]{};
        UINT length = GetSystemDirectoryW(system, MAX_PATH);
        if (!length || length >= MAX_PATH) throw std::runtime_error("Cannot locate the Windows system directory.");
        for (const wchar_t *name : {L"visa64.dll", L"visa32.dll"}) {
            std::wstring candidate(system, length);
            candidate += L"\\";
            candidate += name;
            library_ = LoadLibraryExW(candidate.c_str(), nullptr, LOAD_LIBRARY_SEARCH_SYSTEM32);
            if (library_) break;
        }
#endif
        if (!library_) throw std::runtime_error("No official 64-bit VISA runtime was found in Windows System32.");
        open_default_rm = symbol<ViOpenDefaultRM>("viOpenDefaultRM");
        find_rsrc = symbol<ViFindRsrc>("viFindRsrc");
        find_next = symbol<ViFindNext>("viFindNext");
        open = symbol<ViOpen>("viOpen");
        close = symbol<ViClose>("viClose");
        set_attribute = symbol<ViSetAttribute>("viSetAttribute");
        write = symbol<ViWrite>("viWrite");
        read = symbol<ViRead>("viRead");
        clear = symbol<ViClear>("viClear");
        gpib_control_ren = symbol<ViGpibControlREN>("viGpibControlREN");
    }
    ~Visa() { if (library_) FreeLibrary(library_); }
    Visa(const Visa &) = delete;
    Visa &operator=(const Visa &) = delete;
private:
    template <typename Function> Function symbol(const char *name) {
        FARPROC raw = GetProcAddress(library_, name);
        if (!raw) throw std::runtime_error(std::string("The installed VISA runtime is missing ") + name + ".");
        static_assert(sizeof(Function) == sizeof(raw), "Windows function pointers must have one representation");
        Function value{};
        std::memcpy(&value, &raw, sizeof(value));
        return value;
    }
};

struct ParsedResource { bool gpib; std::string value; };
static bool decimal_address(const std::string &value) {
    if (value.empty() || value.size() > 2 || !std::all_of(value.begin(), value.end(), [](unsigned char c) { return std::isdigit(c) != 0; })) return false;
    unsigned address = static_cast<unsigned>(std::stoul(value));
    return address >= 1 && address <= 30;
}
static ParsedResource parse_resource(const std::string &resource) {
    if (resource.empty() || resource.size() >= VI_FIND_BUFLEN || resource.find_first_of("\r\n\t") != std::string::npos)
        throw std::runtime_error("Invalid VISA resource string.");
    std::vector<std::string> fields;
    for (std::size_t from = 0;;) {
        std::size_t end = resource.find("::", from);
        fields.push_back(resource.substr(from, end == std::string::npos ? end : end - from));
        if (end == std::string::npos) break;
        from = end + 2;
    }
    auto equal = [](const std::string &a, const char *b) {
        return a.size() == std::strlen(b) && std::equal(a.begin(), a.end(), b, [](unsigned char x, unsigned char y) { return std::toupper(x) == std::toupper(y); });
    };
    if (fields.size() == 3 && equal(fields[0], "GPIB0") && equal(fields[2], "INSTR") && decimal_address(fields[1]))
        return {true, resource};
    auto resource_id = [](const std::string &value) -> unsigned {
        std::string digits = value;
        bool hexadecimal = digits.size() > 2 && digits[0] == '0' && (digits[1] == 'x' || digits[1] == 'X');
        if (hexadecimal) digits.erase(0, 2);
        if (digits.empty() || digits.size() > 5 || !std::all_of(digits.begin(), digits.end(), [hexadecimal](unsigned char c) { return hexadecimal ? std::isxdigit(c) != 0 : std::isdigit(c) != 0; }))
            throw std::runtime_error("Invalid VISA USB vendor or product ID.");
        unsigned parsed = static_cast<unsigned>(std::stoul(digits, nullptr, hexadecimal ? 16 : 10));
        if (parsed > 0xffff) throw std::runtime_error("Invalid VISA USB vendor or product ID.");
        return parsed;
    };
    bool usb_shape = (fields.size() == 5 && equal(fields[4], "INSTR")) ||
                     (fields.size() == 6 && equal(fields[4], "0") && equal(fields[5], "INSTR"));
    if (usb_shape && equal(fields[0], "USB0") && !fields[3].empty() &&
        std::all_of(fields[3].begin(), fields[3].end(), [](unsigned char c) { return std::isalnum(c) || c == '_' || c == '-'; })) {
        unsigned vid = resource_id(fields[1]), pid = resource_id(fields[2]);
        char ids[32]{};
        int count = std::snprintf(ids, sizeof(ids), "USB0::0x%04X::0x%04X::", vid, pid);
        if (count < 0) throw std::runtime_error("Cannot normalize VISA USB resource.");
        std::string canonical(ids);
        canonical += fields[3];
        canonical += "::INSTR";
        if (canonical.size() >= VI_FIND_BUFLEN) throw std::runtime_error("VISA USB resource is too long.");
        return {false, canonical};
    }
    throw std::runtime_error("Unsupported VISA resource. Select a USB0 or GPIB0 resource returned by Refresh.");
}

class Session {
    Visa &visa_;
    ViSession rm_ = VI_NULL, instrument_ = VI_NULL;
    bool gpib_ = false, poisoned_ = false;
public:
    Session(Visa &visa, const ParsedResource &resource) : visa_(visa), gpib_(resource.gpib) {
        check(visa_.open_default_rm(&rm_), "resource manager open");
        try {
            check(visa_.open(rm_, resource.value.c_str(), VI_EXCLUSIVE_LOCK, 2000, &instrument_), "instrument open");
            check(visa_.set_attribute(instrument_, VI_ATTR_SEND_END_EN, static_cast<ViAttrState>(1)), "send-end enable");
            check(visa_.set_attribute(instrument_, VI_ATTR_TERMCHAR_EN, static_cast<ViAttrState>(0)), "termination-character disable");
        }
        catch (...) {
            if (instrument_ != VI_NULL) { (void)visa_.close(instrument_); instrument_ = VI_NULL; }
            (void)visa_.close(rm_); rm_ = VI_NULL; throw;
        }
    }
    ~Session() { shutdown(); }
    void timeout(unsigned value) { check(visa_.set_attribute(instrument_, VI_ATTR_TMO_VALUE, static_cast<ViAttrState>(value)), "timeout set"); }
    void write(const std::string &data) {
        if (poisoned_) throw std::runtime_error("A previous VISA transfer failed. Disconnect and reconnect the selected instrument before sending more SCPI commands.");
        if (data.empty()) throw std::runtime_error("VISA writes must not be empty.");
        ViUInt32 transferred = 0;
        ViStatus status = visa_.write(instrument_, reinterpret_cast<ViBuf>(const_cast<char *>(data.data())), static_cast<ViUInt32>(data.size()), &transferred);
        if (!succeeded(status)) { poisoned_ = true; check(status, "write"); }
        if (transferred != data.size()) { poisoned_ = true; throw std::runtime_error("VISA wrote fewer bytes than requested."); }
    }
    std::string read() {
        if (poisoned_) throw std::runtime_error("A previous VISA transfer failed. Disconnect and reconnect the selected instrument before sending more SCPI commands.");
        std::vector<unsigned char> buffer(MAX_PAYLOAD);
        ViUInt32 transferred = 0;
        ViStatus status = visa_.read(instrument_, buffer.data(), static_cast<ViUInt32>(buffer.size()), &transferred);
        if (!succeeded(status)) { poisoned_ = true; check(status, "read"); }
        if (status == VI_SUCCESS_MAX_CNT || transferred >= buffer.size()) { poisoned_ = true; throw std::runtime_error("VISA read reached the 16 KiB limit; response may be truncated."); }
        if (transferred == 0) { poisoned_ = true; throw std::runtime_error("Empty VISA instrument response."); }
        return std::string(reinterpret_cast<const char *>(buffer.data()), transferred);
    }
    void clear() { check(visa_.clear(instrument_), "clear"); }
    void local() {
        check(visa_.gpib_control_ren(instrument_, gpib_ ? VI_GPIB_REN_ADDRESS_GTL : VI_GPIB_REN_DEASSERT_GTL), "go to local");
    }
    void ren_off() {
        // Do not deassert REN: it is a shared GPIB bus line and another VISA
        // session may still be using it.  Addressed GTL returns only this
        // instrument to local control.
        if (gpib_) local();
        else check(visa_.gpib_control_ren(instrument_, VI_GPIB_REN_DEASSERT), "release USB remote enable");
    }
    void shutdown() noexcept {
        if (instrument_ != VI_NULL) {
            (void)visa_.gpib_control_ren(instrument_, gpib_ ? VI_GPIB_REN_ADDRESS_GTL : VI_GPIB_REN_DEASSERT_GTL);
            (void)visa_.close(instrument_); instrument_ = VI_NULL;
        }
        if (rm_ != VI_NULL) { (void)visa_.close(rm_); rm_ = VI_NULL; }
    }
};

static std::string list(Visa &visa) {
    ViSession rm = VI_NULL;
    check(visa.open_default_rm(&rm), "resource manager open");
    std::string result;
    std::vector<std::string> seen;
    try {
        for (const char *pattern : {"USB?*INSTR", "GPIB?*INSTR"}) {
            ViFindList find = VI_NULL; ViUInt32 count = 0; char resource[VI_FIND_BUFLEN]{};
            ViStatus status = visa.find_rsrc(rm, pattern, &find, &count, resource);
            if (status == VI_ERROR_RSRC_NFOUND) continue;
            if (succeeded(status)) {
                if (count > 256) throw std::runtime_error("VISA resource enumeration returned too many resources.");
                try {
                    auto append = [&](const char *name) {
                        ParsedResource parsed{false, ""};
                        try {
                            parsed = parse_resource(name);
                        } catch (const std::runtime_error &) {
                            // VISA may enumerate interfaces outside the narrow
                            // portable resource grammar. They are not opened.
                            return;
                        }
                        if (std::find(seen.begin(), seen.end(), parsed.value) != seen.end()) return;
                        std::string row = parsed.value + "\tOfficial VISA\n";
                        if (result.size() + row.size() > MAX_PAYLOAD) throw std::runtime_error("VISA resource list exceeds 16 KiB.");
                        seen.push_back(parsed.value); result += row;
                    };
                    append(resource);
                    for (ViUInt32 i = 1; i < count; ++i) { std::memset(resource, 0, sizeof(resource)); check(visa.find_next(find, resource), "resource enumeration"); append(resource); }
                    check(visa.close(find), "resource enumeration close");
                } catch (...) { (void)visa.close(find); throw; }
            } else check(status, "resource enumeration");
        }
        check(visa.close(rm), "resource manager close");
    } catch (...) { if (rm != VI_NULL) (void)visa.close(rm); throw; }
    return result;
}

int main(int argc, char **argv) {
    binary_stdio();
    try {
        if (argc != 2) throw std::runtime_error("Expected --available, --list, or one VISA resource.");
        if (!std::strcmp(argv[1], "--available")) {
            try { Visa visa; ViSession rm = VI_NULL; check(visa.open_default_rm(&rm), "resource manager open"); check(visa.close(rm), "resource manager close"); std::cout << "1\n"; }
            catch (...) { std::cout << "0\n"; }
            return 0;
        }
        if (!std::strcmp(argv[1], "--list")) {
            // No runtime is an ordinary condition: direct WinUSB discovery
            // remains available and Refresh must not show a warning each time.
            try { Visa visa; reply("OK", list(visa)); }
            catch (const std::exception &error) {
                if (std::string(error.what()) == "No official 64-bit VISA runtime was found in Windows System32.") reply("OK");
                else throw;
            }
            return 0;
        }
        Visa visa;
        Session session(visa, parse_resource(argv[1]));
        reply("OK");
        std::string line;
        while (true) {
            try {
                if (!read_bounded_line(line)) break;
            } catch (const std::exception &error) {
                reply("ERR", error.what());
                continue;
            }
            try {
                std::istringstream request(line);
                std::string operation, encoded, extra; long long address = -1, timeout_ms = 0;
                if (!(request >> operation >> address >> timeout_ms >> encoded) || (request >> extra) ||
                    address < 0 || address > 30 || timeout_ms < 1 || timeout_ms > 3600000)
                    throw std::runtime_error("Invalid VISA helper request.");
                std::string data = unhex(encoded), response;
                session.timeout(static_cast<unsigned>(timeout_ms));
                if (operation == "EXIT") { session.shutdown(); reply("OK"); return 0; }
                if (operation == "LOCAL") session.local();
                else if (operation == "REN") { if (data != "0") throw std::runtime_error("Invalid VISA REN request."); session.ren_off(); }
                else if (operation == "CLEAR") session.clear();
                else if (operation == "WRITE") session.write(data);
                else if (operation == "READ") { if (!data.empty()) throw std::runtime_error("VISA READ does not accept a payload."); response = session.read(); }
                else if (operation == "QUERY") { session.write(data); response = session.read(); }
                else throw std::runtime_error("Unsupported VISA helper operation.");
                reply("OK", response);
            } catch (const std::exception &error) { reply("ERR", error.what()); }
        }
        return 0;
    } catch (const std::exception &error) { reply("ERR", error.what()); return 1; }
}
#endif

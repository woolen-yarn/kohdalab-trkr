// SPDX-License-Identifier: GPL-2.0-only
// Userspace adaptation of Linux v6.17 drivers/staging/gpib/agilent_82357a
// and include/tms9914.h, copyright (C) 2002, 2004 Frank Mori Hess.
// Modified 2026-10-14: retain only the operational 82357B protocol. Firmware
// acquisition and Cypress FX2 RAM upload were removed from this adaptation.
// This subset supports one 82357B, controller address 0, primary addressing.
#pragma once
#include <libusb-1.0/libusb.h>
#include <chrono>
#include <cstdlib>
#include <iostream>
#include <initializer_list>
#include <stdexcept>
#include <string>
#include <thread>
#include <vector>

class Agilent82357B {
    libusb_context *context_ = nullptr;
    libusb_device_handle *handle_ = nullptr;
    bool claimed_ = false, initialized_ = false, poisoned_ = false;
    unsigned timeout_ = 10000;
    unsigned char hardware_ = 0;
    using Bytes = std::vector<unsigned char>;
    using Clock = std::chrono::steady_clock;

    static void check(int result, const char *operation) {
        if (result < 0) throw std::runtime_error(std::string("82357B ") + operation + ": " + libusb_error_name(result));
    }
    void ensureHealthy() {
        if (poisoned_) throw std::runtime_error("82357B communication failed; reconnect before further commands.");
    }
    void out(Bytes data) {
        int actual = 0;
        check(libusb_bulk_transfer(handle_, 0x06, data.data(), static_cast<int>(data.size()), &actual, timeout_), "USB write");
        if (actual != static_cast<int>(data.size())) throw std::runtime_error("82357B short USB write");
    }
    Bytes in(int capacity) {
        Bytes data(capacity);
        int actual = 0;
        check(libusb_bulk_transfer(handle_, 0x82, data.data(), capacity, &actual, timeout_), "USB read");
        if (actual < 0 || actual > capacity) throw std::runtime_error("82357B invalid USB length");
        data.resize(actual);
        return data;
    }
    void registers(std::initializer_list<std::pair<unsigned char, unsigned char>> pairs) {
        Bytes data{4, static_cast<unsigned char>(pairs.size())};
        for (auto pair : pairs) { data.push_back(pair.first); data.push_back(pair.second); }
        out(data);
        auto reply = in(32);
        if (reply.size() < 2 || reply[0] != 0xfb || reply[1]) throw std::runtime_error("82357B register write rejected");
    }
    unsigned char reg(unsigned char address) {
        out({5, 1, address});
        auto reply = in(32);
        if (reply.size() != 3 || reply[0] != 0xfa || reply[1]) throw std::runtime_error("82357B invalid register response");
        return reply[2];
    }
    static std::string hexByte(unsigned char value) {
        const char *digits = "0123456789abcdef";
        return std::string{"0x"} + digits[value >> 4] + digits[value & 15];
    }
    void traceState(const char *phase) {
        if (!std::getenv("KOHDALAB_IV_82357B_TRACE")) return;
        unsigned char bus = reg(3);
        // TI9914 ADSR reads can access the data register while listener-active
        // with ATN released (Linux agilent_82357a_read workaround). Diagnostics
        // must not consume an instrument's first response byte.
        std::string address = (bus & 0x80) ? hexByte(reg(2)) : "skipped (ATN released)";
        unsigned char hardware = reg(0x0a);
        std::cerr << "82357B " << phase << ": BSR=" << hexByte(bus) << ", ADSR=" << address
            << ", HW_CONTROL=" << hexByte(hardware) << std::endl;
    }
    void auxiliary(unsigned char value) { registers({{3, value}}); }
    void takeControl() {
        traceState("before TCA");
        auxiliary(0x0c); // AUX_TCA
        unsigned char bus = 0;
        auto deadline = Clock::now() + std::chrono::milliseconds(100);
        do {
            // ADSR can be corrupted on 82357B; BSR reports the physical ATN line.
            // Do not use an unreliable ADSR bit as a prerequisite for command I/O.
            bus = reg(3);
            if (bus & 0x80) { traceState("after TCA"); return; } // BSR_ATN_BIT
            std::this_thread::sleep_for(std::chrono::milliseconds(1));
        } while (Clock::now() < deadline);
        throw std::runtime_error("82357B could not assert ATN (BSR=" + hexByte(bus) +
            ", ADSR=" + std::string("skipped (ATN not asserted)") + ", HW_CONTROL=" + hexByte(reg(0x0a)) + ")");
    }
    void standby() {
        // Linux common/iblib.c calls ibgts before driver write/read.
        // NO_ADDRESS transfers assume the controller has already released ATN.
        auxiliary(0x0b); // AUX_GTS
        auto deadline = Clock::now() + std::chrono::milliseconds(100);
        unsigned char bus = 0;
        do {
            bus = reg(3);
            if (!(bus & 0x80)) { traceState("after standby"); return; }
            std::this_thread::sleep_for(std::chrono::milliseconds(1));
        } while (Clock::now() < deadline);
        throw std::runtime_error("82357B could not release ATN (BSR=" + hexByte(bus) + ")");
    }
    void abortTransfer() noexcept {
        unsigned char response[2]{};
        libusb_control_transfer(handle_, 0xc0, 4, 0xa0, 0, response, 2, 100);
    }
    void writeBytes(const Bytes &bytes, bool command) {
        // Drain queued notifications so an old completion cannot finish this write.
        unsigned char notification[8];
        int actual = 0;
        bool drained = false;
        for (int i = 0; i < 16; ++i) {
            int result = libusb_interrupt_transfer(handle_, 0x88, notification, 8, &actual, 1);
            if (result == LIBUSB_ERROR_TIMEOUT) { drained = true; break; }
            check(result, "interrupt drain");
        }
        if (!drained) throw std::runtime_error("82357B interrupt queue did not drain");
        Bytes message{1, 0, 0, static_cast<unsigned char>(command ? 0x1e : 0x0b)};
        unsigned length = static_cast<unsigned>(bytes.size());
        for (int shift = 0; shift < 32; shift += 8) message.push_back(length >> shift);
        message.insert(message.end(), bytes.begin(), bytes.end());
        out(message);
        auto deadline = Clock::now() + std::chrono::milliseconds(timeout_);
        while (true) {
            auto remaining = std::chrono::duration_cast<std::chrono::milliseconds>(deadline - Clock::now()).count();
            if (remaining <= 0) throw std::runtime_error("82357B GPIB write timed out");
            check(libusb_interrupt_transfer(handle_, 0x88, notification, 8, &actual, static_cast<unsigned>(remaining)), "write completion");
            if (actual < 1) throw std::runtime_error("82357B empty write notification");
            if (notification[0] & 2) break;
        }
        unsigned char status[8]{};
        int result = libusb_control_transfer(handle_, 0xc0, 4, 0xb0, 0, status, 8, 100);
        check(result, "write status");
        std::string raw;
        const char *digits = "0123456789abcdef";
        for (int i = 0; i < result && i < 8; ++i) {
            if (i) raw += ' ';
            raw += digits[status[i] >> 4]; raw += digits[status[i] & 15];
        }
        std::string diagnostic = "USB bytes=" + std::to_string(result) + ", raw=" + raw;
        if (std::getenv("KOHDALAB_IV_82357B_TRACE"))
            std::cerr << "82357B " << (command ? "command" : "data") << " completion: expected=" << length << ", " << diagnostic << std::endl;
        // Unlike WR_REGS/RD_REGS/ABORT, XFER_STATUS has no documented ~command
        // reply prefix. Linux-GPIB consumes the little-endian count at [2..5]
        // after the write-complete interrupt; do not invent a 0x4f requirement.
        // The physical 82357B returns six bytes (flags + 32-bit count).
        // Eight is the buffer capacity, not the required USB response length.
        if (result < 6 || result > 8) throw std::runtime_error("82357B invalid write status (" + diagnostic + ")");
        unsigned count = static_cast<unsigned>(status[2]) | (static_cast<unsigned>(status[3]) << 8) |
            (static_cast<unsigned>(status[4]) << 16) | (static_cast<unsigned>(status[5]) << 24);
        if (count != length) throw std::runtime_error("82357B incomplete GPIB write (expected=" +
            std::to_string(length) + ", actual=" + std::to_string(count) + ", " + diagnostic + ")");
    }
    void command(std::initializer_list<unsigned char> bytes) { takeControl(); writeBytes(Bytes(bytes), true); }
    template <class Function> auto transaction(Function fn) -> decltype(fn()) {
        ensureHealthy();
        try { return fn(); }
        catch (...) { poisoned_ = true; abortTransfer(); throw; }
    }

public:
    Agilent82357B() = default;
    Agilent82357B(const Agilent82357B &) = delete;
    ~Agilent82357B() { close(); }
    void setTimeout(unsigned usec) { timeout_ = (usec + 999) / 1000; }
    void open();
    void diagnose() {
        enableRemote(true);
        traceState("before controller test");
        takeControl();
        standby(); // release ATN again; no GPIB data/command bytes.
        std::cout << "82357B controller test passed. No SCPI or GPIB data was sent." << std::endl;
    }
    void close() noexcept {
        if (initialized_) {
            try { auxiliary(0x10); } catch (...) {} // release REN even after a failed operation
            try { registers({{3, 0x80}, {0x0a, static_cast<unsigned char>(hardware_ & ~1)},
                {0x0d, 0}, {0, 0}, {1, 0}, {0x0b, 0}}); } catch (...) {}
            initialized_ = false;
        }
        if (claimed_) { libusb_release_interface(handle_, 0); claimed_ = false; }
        if (handle_) { libusb_close(handle_); handle_ = nullptr; }
        if (context_) { libusb_exit(context_); context_ = nullptr; }
    }
    void enableRemote(bool enable) { auxiliary(enable ? 0x90 : 0x10); }
    bool listenerPresent(int address) {
        return transaction([&] {
            command({0x3f, 0x40, static_cast<unsigned char>(0x20 + address)});
            standby(); // controller talker, candidate listener
            std::this_thread::sleep_for(std::chrono::milliseconds(1));
            bool present = reg(3) & 0x20; // BSR_NDAC_BIT
            command({0x3f, 0x5f});
            return present;
        });
    }
    void goToLocal(int address) {
        // Cleanup is allowed after a failed transfer; normal SCPI is blocked.
        command({0x3f, static_cast<unsigned char>(0x20 + address), 1, 0x3f, 0x5f});
    }
    void deviceClear(int address) {
        transaction([&] { command({0x3f, static_cast<unsigned char>(0x20 + address), 4}); command({0x3f, 0x5f}); });
    }
    void send(int address, const std::string &data) {
        transaction([&] {
            if (data.empty() || data.size() > 1024 * 1024) throw std::runtime_error("82357B invalid message size");
            command({0x40, 0x3f, static_cast<unsigned char>(0x20 + address)});
            standby();
            writeBytes(Bytes(data.begin(), data.end()), false);
            command({0x3f, 0x5f});
        });
    }
    std::string read(int address) {
        return transaction([&] {
            command({0x3f, 0x20, static_cast<unsigned char>(0x40 + address)});
            // Follow Linux-GPIB's receive path: GTS immediately followed by
            // the USB READ request. Do not interleave status-register requests
            // once the instrument can start transmitting its response.
            auxiliary(0x0b); // AUX_GTS; write ACK is checked by registers()
            // NO_ADDRESS | END_ON_EOI. Maximum ASCII response 4096 bytes.
            out({3, 0, 0, 3, 0, 0x10, 0, 0, 0});
            auto response = in(4097);
            if (response.empty()) throw std::runtime_error("82357B missing read status");
            unsigned char status = response.back();
            response.pop_back();
            // Never accept a timeout, partial response, or stale data as a measurement.
            if (!(status & 1) || (status & 0xd6)) throw std::runtime_error("82357B incomplete GPIB response (status " + std::to_string(status) + ")");
            takeControl(); // TI9914 listener-active workaround from Linux-GPIB
            command({0x3f, 0x5f});
            while (!response.empty() && (response.back() == '\r' || response.back() == '\n')) response.pop_back();
            return std::string(response.begin(), response.end());
        });
    }
    std::string query(int address, const std::string &data) { send(address, data); return read(address); }
};

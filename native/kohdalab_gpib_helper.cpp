// SPDX-License-Identifier: GPL-2.0-or-later
// Framed protocol for the separate KAME / linux-gpib userspace driver process.
#include "NiGpibDriver.h"
#include "agilent_82357b.hpp"
#include "stdio_mode.hpp"
#include <chrono>
#include <iostream>
#include <sstream>
#include <stdexcept>

static std::string hex(const std::string &value) {
    static const char *digits = "0123456789abcdef";
    std::string result;
    for (unsigned char byte : value) {
        result += digits[byte >> 4];
        result += digits[byte & 15];
    }
    return result;
}

static std::string unhex(const std::string &value) {
    if (value == "-") return "";
    if (value.size() % 2) throw std::runtime_error("Invalid hex payload");
    std::string result;
    for (size_t i = 0; i < value.size(); i += 2) {
        auto nibble = [](char c) -> int {
            if (c >= '0' && c <= '9') return c - '0';
            if (c >= 'a' && c <= 'f') return c - 'a' + 10;
            throw std::runtime_error("Invalid hex payload");
        };
        result += static_cast<char>((nibble(value[i]) << 4) | nibble(value[i + 1]));
    }
    return result;
}

static void reply(const char *status, const std::string &message = "") {
    std::cout << "KIV\t" << status << '\t' << hex(message) << std::endl;
}

// 0: none, 1: NI, 2: Agilent. Never silently select the wrong bus.
static int adapter_kind(bool presence_only = false) {
    libusb_context *context = nullptr;
    int result = libusb_init(&context);
    if (result < 0) throw std::runtime_error("Cannot initialize USB adapter discovery");
    libusb_device **devices = nullptr;
    ssize_t count = libusb_get_device_list(context, &devices);
    int found = 0, matches = 0;
    if (count >= 0) {
        for (ssize_t i = 0; i < count; ++i) {
            libusb_device_descriptor descriptor{};
            if (libusb_get_device_descriptor(devices[i], &descriptor) != 0) continue;
            if (descriptor.idVendor == 0x3923) {
                for (unsigned pid : {0x702a, 0x709b, 0x7618, 0x725c, 0x725d})
                    if (descriptor.idProduct == pid) { found = 1; ++matches; }
            } else if (descriptor.idVendor == 0x0957 &&
                (descriptor.idProduct == 0x0518 || descriptor.idProduct == 0x0718)) { found = 2; ++matches; }
        }
        libusb_free_device_list(devices, 1);
    }
    libusb_exit(context);
    if (count < 0) throw std::runtime_error("Cannot enumerate USB adapters");
    if (matches > 1 && !presence_only) throw std::runtime_error("Connect only one supported USB-GPIB adapter at a time.");
    return found;
}

class Driver {
    NiGpibDriver ni_{0, 10000000};
    Agilent82357B agilent_;
    bool agilent = false;
public:
    void open() {
        int kind = adapter_kind();
        if (!kind) throw std::runtime_error("No NI USB-GPIB or Agilent 82357B adapter found.");
        agilent = kind == 2;
        if (agilent) agilent_.open();
        else if (!ni_.open()) throw std::runtime_error("Cannot open NI USB-GPIB adapter. Check USB connection and close other GPIB software.");
        std::cerr << "Using " << (agilent ? "Agilent 82357B" : "NI USB-GPIB") << std::endl;
    }
    void setTimeout(unsigned us) { if (agilent) agilent_.setTimeout(us); else ni_.setTimeout(us); }
    void close() { if (agilent) agilent_.close(); else ni_.close(); }
    void enableRemote(bool enable) { if (agilent) agilent_.enableRemote(enable); else ni_.enableRemote(enable); }
    bool listenerPresent(int addr) { return agilent ? agilent_.listenerPresent(addr) : ni_.listenerPresent(addr); }
    void goToLocal(int addr) { if (agilent) agilent_.goToLocal(addr); else ni_.goToLocal(addr); }
    void deviceClear(int addr) { if (agilent) agilent_.deviceClear(addr); else ni_.deviceClear(addr); }
    void send(int addr, const std::string &data) { if (agilent) agilent_.send(addr, data); else ni_.send(addr, data); }
    std::string query(int addr, const std::string &data) { return agilent ? agilent_.query(addr, data) : ni_.query(addr, data); }
    std::string read(int addr) { return agilent ? agilent_.read(addr) : ni_.read(addr); }
    void diagnose() {
        if (!agilent) throw std::runtime_error("--diagnose is only available for Agilent 82357B.");
        agilent_.diagnose();
    }
};

int main(int argc, char **argv) {
    binary_stdio();
    if (argc == 2 && std::string(argv[1]) == "--available") {
        try { std::cout << (adapter_kind(true) ? "1" : "0") << std::endl; return 0; }
        catch (const std::exception &error) { reply("ERR", error.what()); return 1; }
    }
    bool diagnose = argc == 2 && std::string(argv[1]) == "--diagnose";
    if (diagnose) {
#ifdef _WIN32
        _putenv_s("KOHDALAB_IV_82357B_TRACE", "1");
#else
        setenv("KOHDALAB_IV_82357B_TRACE", "1", 1);
#endif
    }
    Driver driver;
    try { driver.open(); }
    catch (const std::exception &error) {
        if (diagnose || (argc >= 2 && std::string(argv[1]) == "--idn"))
            std::cerr << error.what() << std::endl;
        else reply("ERR", error.what());
        return 1;
    }
    if (diagnose) {
        try {
            driver.diagnose();
            driver.enableRemote(false); driver.close(); return 0;
        } catch (const std::exception &error) {
            try { driver.enableRemote(false); } catch (...) {}
            std::cerr << error.what() << std::endl; return 1;
        }
    }
    if (argc >= 3 && std::string(argv[1]) == "--idn") {
        try {
            for (int i = 2; i < argc; ++i) {
                std::string argument(argv[i]);
                size_t consumed = 0;
                int address = std::stoi(argument, &consumed);
                if (consumed != argument.size() || address < 1 || address > 30) throw std::runtime_error("Address must be 1–30");
                driver.enableRemote(true);
                std::cout << "GPIB " << address << ": " << driver.query(address, "*IDN?") << std::endl;
                driver.goToLocal(address);
            }
            driver.enableRemote(false); driver.close(); return 0;
        } catch (const std::exception &error) {
            try { driver.enableRemote(false); } catch (...) {}
            std::cerr << error.what() << std::endl; return 1;
        }
    }
    reply("OK");
    std::string line;
    while (std::getline(std::cin, line)) {
        try {
            std::istringstream request(line);
            std::string operation, payload, extra;
            int address = 0, timeout = 0;
            if (!(request >> operation >> address >> timeout >> payload) ||
                (request >> extra) || address < 1 || address > 30 ||
                timeout < 1 || timeout > 3600000)
                throw std::runtime_error("Invalid GPIB request");
            std::string data = unhex(payload);
            driver.setTimeout(static_cast<unsigned int>(timeout) * 1000);
            std::string response;
            if (operation == "LIST") {
                // Presence detection also finds non-SCPI devices; no *IDN? is sent.
                driver.setTimeout(500000);
                auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(10);
                for (int candidate = 1; candidate <= 30; ++candidate) {
                    if (std::chrono::steady_clock::now() > deadline)
                        throw std::runtime_error("GPIB discovery timed out; check the bus and adapter.");
                    if (driver.listenerPresent(candidate))
                        response += "GPIB0::" + std::to_string(candidate) + "::INSTR\n";
                }
            } else if (operation == "EXIT") {
                driver.enableRemote(false);
                driver.close();
                reply("OK");
                return 0;
            } else if (operation == "REN") {
                if (data != "0") throw std::runtime_error("Invalid REN request");
                driver.enableRemote(false);
            } else if (operation == "LOCAL") {
                driver.goToLocal(address);
            } else if (operation == "CLEAR") {
                driver.deviceClear(address);
            } else if (operation == "WRITE" || operation == "QUERY" || operation == "READ") {
                driver.enableRemote(true);
                if (operation == "WRITE") driver.send(address, data);
                else if (operation == "QUERY") response = driver.query(address, data);
                else response = driver.read(address);
                if (operation != "WRITE" && response.empty())
                    throw std::runtime_error("Empty GPIB response");
            } else {
                throw std::runtime_error("Unknown GPIB operation");
            }
            reply("OK", response);
        } catch (const std::exception &error) {
            reply("ERR", error.what());
        }
    }
    driver.enableRemote(false);
    driver.close();
    return 0;
}

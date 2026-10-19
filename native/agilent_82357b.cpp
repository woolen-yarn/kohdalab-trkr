// SPDX-License-Identifier: GPL-2.0-only
// Adapted from Linux v6.17 Agilent 82357a, copyright (C) 2002, 2004 Frank Mori Hess.
// Modified 2026-10-14: firmware acquisition/upload removed; initialized adapters only.
#include "agilent_82357b.hpp"

void Agilent82357B::open() {
    check(libusb_init(&context_), "initialization");
    libusb_device **devices = nullptr;
    ssize_t count = libusb_get_device_list(context_, &devices);
    check(static_cast<int>(count), "enumeration");
    bool boot_mode = false;
    for (ssize_t i = 0; i < count; ++i) {
        libusb_device_descriptor descriptor{};
        if (libusb_get_device_descriptor(devices[i], &descriptor) != 0 || descriptor.idVendor != 0x0957) continue;
        if (descriptor.idProduct == 0x0518) { boot_mode = true; continue; }
        if (descriptor.idProduct != 0x0718 || handle_) continue;
        int result = libusb_open(devices[i], &handle_);
        if (result < 0) { libusb_free_device_list(devices, 1); check(result, "open (close other GPIB software)"); }
    }
    libusb_free_device_list(devices, 1);
    if (!handle_) {
        if (boot_mode) throw std::runtime_error(
            "82357B is in boot mode (0957:0518). KohdaLab IV supports only a pre-initialized "
            "82357B (0957:0718). Obtain authorized initialization separately, then retry without disconnecting adapter power.");
        throw std::runtime_error("No operational Agilent 82357B (0957:0718) adapter found.");
    }
    libusb_set_auto_detach_kernel_driver(handle_, 1);
    check(libusb_claim_interface(handle_, 0), "claim interface (close other GPIB software)");
    claimed_ = true;
    traceState("before hardware reset");
    registers({{0x0b, 0x20}, {0x0c, 1}}); // failure LED, hardware reset
    std::this_thread::sleep_for(std::chrono::milliseconds(2));
    traceState("after hardware reset");
    // close() asserts the external TI reset (NOT_TI_RESET=0). On the remote
    // adapter RESET_TO_POWERUP preserves HW_CONTROL=0x3e, so AUX writes made
    // before deasserting that reset were ignored. Release it before chip setup.
    hardware_ = (reg(0x0a) & ~7) | 5;
    // The write may take effect even if its USB acknowledgement is lost.
    initialized_ = true;
    registers({{0x0a, hardware_}});
    std::this_thread::sleep_for(std::chrono::milliseconds(2));
    traceState("after hardware reset release");
    registers({{3, 0x80}, {3, 5}, {3, 4}, {3, 0x0a}, {3, 9}, {3, 0x18}, {3, 1}, {3, 0x0e},
        {3, 0x15}, {3, 0x17}, {0x0e, 38}, {4, 0}, {6, 0}, {5, 0}, {0x0d, 1},
        {0, 0x30}, {1, 2}, {3, 0}, {0x0b, 1}});
    traceState("after register initialization");
    hardware_ |= 2;
    registers({{3, 0x11}, {0x0a, hardware_}}); // system controller
    traceState("after system controller request");
    auxiliary(0x8f); // assert IFC, become controller in charge
    std::this_thread::sleep_for(std::chrono::microseconds(150));
    traceState("IFC asserted");
    auxiliary(0x0f);
    traceState("IFC released");
}

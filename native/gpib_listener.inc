// SPDX-License-Identifier: GPL-2.0-or-later
// Appended to the pinned driver by build_native_gpib.sh.
// IEEE-488 Find Listeners: address the controller as talker, address the
// candidate as listener, release ATN, and inspect NDAC. No data bytes are sent.
bool NiGpibDriver::listenerPresent(int address)
{
    if (address < 1 || address > 30)
        throw std::runtime_error("Invalid GPIB listener address");
    if (!g_ni_gpib_interface || !g_ni_gpib_interface->line_status ||
        !g_ni_gpib_interface->go_to_standby)
        throw std::runtime_error("Adapter does not support GPIB listener detection");
    try {
        cmd({ UNL, MTA((uint32_t)board_pad_), MLA((uint32_t)address) });
        int result = g_ni_gpib_interface->go_to_standby(&board_);
        if (result < 0)
            throw std::runtime_error("GPIB listener standby error: " + std::string(strerror(-result)));
        usleep(1000);
        int lines = -EBUSY;
        for (unsigned attempt = 0; attempt < 10; ++attempt) {
            lines = g_ni_gpib_interface->line_status(&board_);
            if (lines != -EBUSY) break;
            usleep(1000);
        }
        if (lines < 0)
            throw std::runtime_error("GPIB listener line-status error: " + std::string(strerror(-lines)));
        if (!(lines & ValidNDAC))
            throw std::runtime_error("Adapter cannot monitor GPIB NDAC");
        bool present = (lines & BusNDAC) != 0;
        cmd({ UNL, UNT });
        return present;
    } catch (...) {
        try { cmd({ UNL, UNT }); } catch (...) {}
        throw;
    }
}

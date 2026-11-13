# Corresponding native dependency source

`ni-gpib` contains KAME's userspace Linux-GPIB driver from commit
`b413cc7102e984fa8e5e13bd4993549a6e4584cc`:
https://github.com/northriv/KAME/tree/b413cc7102e984fa8e5e13bd4993549a6e4584cc/modules/charinterface/usermode-linux-gpib

Local modifications add `setTimeout`, addressed Go-To-Local, and listener
discovery to NiGpibDriver; see `native/gpib_listener.inc`. Windows compatibility
fixes, if needed, are retained directly in the supplied source. The NI wrapper permits GPL-2.0-or-later; inherited Linux-GPIB files retain
their individual GPLv2 notices (including GPL-2.0-only headers). GPLv2 is
the common distribution version. Preserve notices in each file and
`ni-gpib/linux-gpib/COPYING`.

`libusb-1.0.30.tar.bz2` is the unmodified upstream release source:
https://github.com/libusb/libusb/releases/tag/v1.0.30
SHA256: `fea36f34f9156400209595e300840767ab1a385ede1dc7ee893015aea9c6dbaf`.
Its COPYING license is LGPL-2.1-or-later. Complete source is supplied because
both Mac and Windows helpers statically link this library. Full application
source and relinking instructions in `native/REBUILDING.md` permit replacement
of the LGPL library. Rebuild with
`scripts/build_native.sh` in MSYS2 UCRT64 using GCC, Make, and Bash.

The 82357B adaptation is in `native/agilent_82357b.cpp/.hpp` (GPL-2.0-only).
It is based on Linux v6.17 `drivers/staging/gpib/agilent_82357a` and
`drivers/staging/gpib/include/tms9914.h`:
https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/tree/drivers/staging/gpib?h=v6.17
Its source, USB protocol and initialization changes are included here.
Firmware rights have not been established; download and upload are removed.
Only an already initialized 82357B is supported. See root REDISTRIBUTION.md.

Windows HS+ initialization omits optional analyzer request 0xf8 (interface 1).
WinUSB rewrites interface-recipient wIndex to the selected interface; sending
this request through the GPIB interface (0) stalls. The upstream function notes
that these analyzer-related requests are not required for GPIB operation.
Device/LED requests and their failures remain checked. The Windows installer
still assigns only interface 0 and does not replace the analyzer driver.
The original POSIX initialization is retained.

The combined GPIB executable is distributed under GPLv2 because the Agilent
part is GPL-2.0-only. This repository does not bundle libwdi, USB Setup or any
proprietary firmware. See root REDISTRIBUTION.md and native/REBUILDING.md.

// SPDX-License-Identifier: MIT
#pragma once
#ifdef _WIN32
#include <fcntl.h>
#include <io.h>
#include <cstdio>
#endif
inline void binary_stdio() {
#ifdef _WIN32
    _setmode(_fileno(stdin), _O_BINARY);
    _setmode(_fileno(stdout), _O_BINARY);
#endif
}

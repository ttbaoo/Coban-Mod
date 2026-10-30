// SPDX-License-Identifier: GPL-2.0-or-later
#include "layer_read.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>

static uint32_t be32(const uint8_t *p) {
    return ((uint32_t)p[0] << 24) | ((uint32_t)p[1] << 16) | ((uint32_t)p[2] << 8) | p[3];
}

int main(void) {
    // Canaries verify that encoding changes only the payload, not VIA headers
    // or memory after the report. Short/oversize messages must stay untouched.
    uint8_t report[34], saved[34];
    memset(report, 0xa5, sizeof(report));
    report[0] = 8; report[1] = 0; report[2] = COBAN_LAYER_READ_VALUE_ID;
    memcpy(saved, report, sizeof(report));
    for (uint8_t size = 0; size < 40; ++size) {
        if (size == 29) continue;
        assert(!coban_layer_read_reply(report + 3, size, 2, 1, 4, 0));
        assert(memcmp(report, saved, sizeof(report)) == 0);
    }
    assert(!coban_layer_read_reply(NULL, 29, 0, 0, 4, 0));
    assert(coban_layer_read_reply(report + 3, 29, 4, 1, 4, 0x12345678));
    assert(memcmp(report, saved, 3) == 0 && report[32] == 0xa5 && report[33] == 0xa5);
    assert(memcmp(report + 3, "CBLR", 4) == 0);
    assert(report[7] == 1 && report[8] == 3 && report[9] == 2 && report[10] == 4);
    assert(be32(report + 11) == 4 && be32(report + 15) == 1 && be32(report + 19) == 0x12345678);
    for (int i = 23; i < 32; ++i) assert(report[i] == 0);
    // This report is consumed independently by the Rust decoder test.
    for (int i = 0; i < 32; ++i) printf("%02x", report[i]);
    puts("");

    // TO cycle, MO/TG multiple layers, auto-return, default layer and layer 31.
    const struct {uint32_t layers, defaults; uint8_t count, expected;} cases[] = {
        {0, 0, 4, 0}, {1, 1, 4, 0}, {2, 1, 4, 1}, {4, 1, 4, 2}, {8, 1, 4, 3},
        {0, 1, 4, 0}, {6, 1, 4, 2}, {2, 8, 4, 3}, {0, 8, 4, 3},
        {UINT32_C(0x80000000), 1, 32, 31}, {16, 1, 4, 0xff}, {0, 1, 0, 0xff}, {0, 1, 33, 0xff}
    };
    for (unsigned i = 0; i < sizeof(cases) / sizeof(cases[0]); ++i) {
        assert(coban_layer_read_reply(report + 3, 29, cases[i].layers, cases[i].defaults, cases[i].count, UINT32_MAX));
        assert(report[9] == cases[i].expected);
        assert(report[8] == (cases[i].expected == 0xff ? 0x83 : 3));
        assert(be32(report + 11) == cases[i].layers && be32(report + 15) == cases[i].defaults);
        assert(be32(report + 19) == UINT32_MAX);
    }
    return 0;
}

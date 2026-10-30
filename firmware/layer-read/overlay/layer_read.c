// SPDX-License-Identifier: GPL-2.0-or-later
#include "layer_read.h"
#include <string.h>

static void write_be32(uint8_t *out, uint32_t value) {
    out[0] = (uint8_t)(value >> 24);
    out[1] = (uint8_t)(value >> 16);
    out[2] = (uint8_t)(value >> 8);
    out[3] = (uint8_t)value;
}

bool coban_layer_read_reply(uint8_t *payload, uint8_t length, uint32_t layers, uint32_t defaults, uint8_t count, uint32_t uptime) {
    if (payload == NULL || length != COBAN_LAYER_READ_PAYLOAD_SIZE) {
        return false;
    }
    memset(payload, 0, COBAN_LAYER_READ_PAYLOAD_SIZE);
    memcpy(payload, "CBLR", 4);
    payload[4] = 1; // Wire version, independent of VIA/firmware version.
    payload[5] = 3; // Bit 0: live masks; bit 1: single-decoder GIF reload fix.
    uint32_t effective = layers | defaults;
    uint8_t highest = 0; // QMK resolves an empty mask to layer 0.
    for (uint8_t bit = 0; bit < 32; ++bit) {
        if (effective & (UINT32_C(1) << bit)) {
            highest = bit;
        }
    }
    if (count == 0 || count > 32 || highest >= count) {
        payload[5] |= 0x80; // Invalid runtime state, host must not select an image.
        highest = 0xff;
    }
    payload[6] = highest;
    payload[7] = count;
    write_be32(payload + 8, layers);
    write_be32(payload + 12, defaults);
    write_be32(payload + 16, uptime);
    // Bytes 20..28 reserved, zero. No change counter is advertised in v1.
    return true;
}

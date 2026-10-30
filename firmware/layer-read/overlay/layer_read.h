// SPDX-License-Identifier: GPL-2.0-or-later
#pragma once

#include <stdbool.h>
#include <stdint.h>

#define COBAN_LAYER_READ_VALUE_ID 0x65
#define COBAN_LAYER_READ_PAYLOAD_SIZE 29

// Stateless encoder: only writes the supplied USB response, never device state.
bool coban_layer_read_reply(uint8_t *payload, uint8_t length, uint32_t layers, uint32_t defaults, uint8_t count, uint32_t uptime);

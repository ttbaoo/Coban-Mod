// SPDX-License-Identifier: GPL-2.0-or-later
#pragma once

// The pinned upstream reserves only 4 KiB after the GIF, but its RP2040
// EEPROM backing store occupies the final 8 KiB. Keep EEPROM where it is,
// and move only the 100 KiB GIF region down by one 4 KiB erase sector.
#define WEAR_LEVELING_BACKING_SIZE 8192
#undef EEROM_CB_GIF_ADDR
#define EEROM_CB_GIF_ADDR (PICO_FLASH_SIZE_BYTES - EEPROM_MAX_GIF_SIZE - WEAR_LEVELING_BACKING_SIZE)

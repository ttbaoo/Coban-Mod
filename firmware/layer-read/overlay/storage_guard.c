// SPDX-License-Identifier: GPL-2.0-or-later
#include "pico.h"
#include "hardware/flash.h"
#include "wear_leveling_rp2040_flash_config.h"

_Static_assert(WEAR_LEVELING_BACKING_SIZE == 8192, "Unexpected EEPROM backing size");
_Static_assert(EEPROM_MAX_GIF_SIZE == 102400, "Unexpected GIF capacity");
_Static_assert(EEROM_CB_GIF_ADDR % FLASH_SECTOR_SIZE == 0, "GIF start must align to erase sectors");
_Static_assert(EEPROM_MAX_GIF_SIZE % FLASH_SECTOR_SIZE == 0, "GIF size must align to erase sectors");
_Static_assert(EEROM_CB_GIF_ADDR + EEPROM_MAX_GIF_SIZE <= WEAR_LEVELING_RP2040_FLASH_BASE, "GIF must not overlap EEPROM");
_Static_assert(WEAR_LEVELING_RP2040_FLASH_BASE + WEAR_LEVELING_BACKING_SIZE <= PICO_FLASH_SIZE_BYTES, "EEPROM exceeds flash");

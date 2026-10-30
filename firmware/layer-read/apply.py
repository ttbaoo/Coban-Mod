#!/usr/bin/env python3
"""Apply the layer GET, GIF reload fix and EEPROM boundary guard to pinned QMK.

No USB access, build invocation, flashing, resetting, or dependency installation.
"""
import argparse
import pathlib
import shutil
import subprocess

BASE_COMMIT = "f2a3ab0484044e88ae1bb9528764fd4ce6f7f409"
HERE = pathlib.Path(__file__).resolve().parent


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("qmk_root", type=pathlib.Path)
    args = parser.parse_args()
    root = args.qmk_root.resolve()
    if not (root / ".git").exists():
        parser.error("Không tìm thấy checkout Git tại qmk_root")
    head = subprocess.check_output(["git", "-C", str(root), "rev-parse", "HEAD"], text=True).strip()
    if head != BASE_COMMIT:
        parser.error(f"Cần đúng commit {BASE_COMMIT}; hiện tại {head}")
    status = subprocess.check_output(["git", "-C", str(root), "status", "--porcelain"], text=True)
    if status.strip():
        parser.error("Checkout có thay đổi cục bộ; cần checkout sạch riêng")
    board = root / "keyboards/coban/pad10a"
    hid = board / "hid/hid.c"
    rules = board / "rules.mk"
    # Refuse to overwrite local work, including an already patched checkout.
    for path in (hid, rules):
        rel = path.relative_to(root).as_posix()
        expected = subprocess.check_output(["git", "-C", str(root), "show", f"HEAD:{rel}"])
        if path.read_bytes() != expected:
            parser.error(f"Tệp đã thay đổi: {rel}; dùng checkout sạch riêng")
    for path in (board / "hid/layer_read.c", board / "hid/layer_read.h", board / "keymaps/coban_mod"):
        if path.exists():
            parser.error(f"Không ghi đè {path}")
    source = hid.read_text()
    include = '#include "raw_hid.h"\n'
    anchor = 'void cb_raw_hid_response_kb(uint8_t *data, uint8_t length) {\n'
    case_anchor = '    switch (*command_id) {\n'
    if source.count(include) != 1 or source.count(anchor) != 1:
        parser.error("Nguồn HID khác cấu trúc đã kiểm tra")
    source = source.replace(include, include + '#include "action_layer.h"\n#include "dynamic_keymap.h"\n#include "timer.h"\n#include "hid/layer_read.h"\n', 1)
    duplicate = """            if (current_screen() == coban_screen_anime) {
                screen_animation_reload();
            }
            screen_background_reload();"""
    single = """            // The active background is already the GIF view in animation mode.
            // A second decoder costs another ~51 KiB and can exhaust RP2040 RAM.
            screen_animation_delete();
            screen_background_reload();"""
    if source.count(duplicate) != 1:
        parser.error("Không tìm thấy nhánh reload GIF đã kiểm tra")
    source = source.replace(duplicate, single, 1)
    before, getter = source.split(anchor)
    if case_anchor not in getter:
        parser.error("Không tìm thấy switch GET")
    case = '''#ifdef VIA_ENABLE
        case COBAN_LAYER_READ_VALUE_ID: {
            // Read the QMK action engine, not the LCD cache or editing layer.
            if (length == 30) {
                coban_layer_read_reply(command_data, length - 1, (uint32_t)layer_state,
                                       (uint32_t)default_layer_state, dynamic_keymap_get_layer_count(), timer_read32());
            }
            break;
        }
#endif
'''
    getter = getter.replace(case_anchor, case_anchor + case, 1)
    # All checks finish before writes. Keep identity, custom config encoding
    # and the upstream default keymap untouched.
    hid.write_text(before + anchor + getter)
    rules.write_text(rules.read_text() + "\n# Coban-mod live layer readback (GET only).\nSRC += hid/layer_read.c\n")
    for name in ("layer_read.c", "layer_read.h"):
        shutil.copyfile(HERE / "overlay" / name, board / "hid" / name)
    keymap = board / "keymaps/coban_mod"
    keymap.mkdir()
    shutil.copyfile(board / "keymaps/default/keymap.c", keymap / "keymap.c")
    shutil.copyfile(HERE / "overlay/config.h", keymap / "config.h")
    shutil.copyfile(HERE / "overlay/storage_guard.c", keymap / "storage_guard.c")
    (keymap / "rules.mk").write_text("VIA_ENABLE = yes\nENCODER_MAP_ENABLE = yes\nSRC += storage_guard.c\n")
    print(f"Đã thêm GET 08 00 65 vào {root}; chưa build/nạp thiết bị")


if __name__ == "__main__":
    main()

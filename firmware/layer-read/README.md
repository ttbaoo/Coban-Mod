# Firmware Coban Pad 10A — CBLR wire v1, bản sửa 1.1

Ngày 29/10/2026. **Bản sửa 1.1 đã viết, build, nạp/verify và vượt 40 lượt reload RAM. Getter đã đọc đúng nhiều vòng lớp. Nghiệm thu tự đổi ảnh cùng layout riêng trên UI/LCD đang thực hiện.** Xem [trạng thái đồng bộ lớp](../../docs/LAYER-SYNC.md) để phân biệt nghiệm thu firmware với app tự đổi ảnh.

Bổ sung một GET cho mục tiêu tự đổi ảnh theo lớp. Dựa trên [nguồn 10A của hãng](https://github.com/RyanDam/qmk_firmware/tree/f2a3ab0484044e88ae1bb9528764fd4ce6f7f409/keyboards/coban/pad10a), commit `f2a3ab0484044e88ae1bb9528764fd4ce6f7f409`. Đây là firmware ứng dụng QMK đầy đủ có thêm getter, không phải một plug-in có thể cài riêng vào firmware gốc đang chạy.

## Nội dung thay đổi

- GET `08 00 65`: trả chỉ số lớp hiệu lực, mask lớp đang bật, mask lớp mặc định, số lớp và uptime. Đọc trực tiếp `layer_state`/`default_layer_state` của QMK tại lúc hỏi, không lấy lớp đang chỉnh trên máy hoặc cache LCD.
- Chữ ký `CBLR`, phiên bản 1, marker request `A5`: host không nhầm echo của firmware cũ thành dữ liệu lớp.
- Không thêm SET lớp, phím giả, callback tự gửi HID, mở matrix, hoặc ghi EEPROM trong getter. VIA vẫn gửi đúng một phản hồi cho một request, tương thích hàng đợi cấu hình hiện có.
- Giữ USB `Coban Pad 10A`, VID/PID `CB3A:C10A`, descriptor và vùng keymap/macro. Keymap build riêng `coban_mod` bật VIA và encoder map; binding mặc định sao chép từ keymap hãng, gồm vòng nhấn núm phải 1→2→3→4→1.
- Sửa chồng vùng GIF/EEPROM trong keymap thử, giải thích bên dưới.
- Bản 1.1 sửa SET-size khi đang ở GIF: xóa animation view cũ, chỉ reload background để không giữ hai decoder/framebuffer làm cạn heap. CBLR trả capability bit 1; app 0.6.5 chỉ bật tự đổi ảnh khi có bit này. Không sửa Quantum/QMK core, GPIO, driver LCD/RTC/RGB/audio hay định dạng custom config.

`overlay/layer_read.c` là bộ mã hóa payload dùng chung cho build ARM và kiểm thử C. `apply.py` thêm nhánh GET vào đúng handler hãng, từ chối khác commit hoặc ghi đè tệp đã sửa. `coban-pad10a-layer-read-v1.1.patch` là bản diff đầy đủ trên commit gốc, có thể review/apply thay cho script; không dùng cả hai trên cùng checkout.

## Giao thức 32 byte

Request: `08 00 65` và 29 byte `A5`. HIDAPI thêm report ID 0 ở ngoài payload như giao thức gốc.

| Offset trong payload USB | Kích thước | Phản hồi |
| --- | --- | --- |
| 0–2 | 3 | `08 00 65` |
| 3–6 | 4 | ASCII `CBLR` |
| 7 | 1 | Phiên bản `01` |
| 8 | 1 | `03` ở bản 1.1: bit 0 lớp hợp lệ, bit 1 đã sửa GIF; bit 7 báo runtime vượt giới hạn |
| 9 | 1 | Lớp hiệu lực, số 0–3 với build này; UI hiển thị cộng 1 |
| 10 | 1 | Số lớp, build này là 4 |
| 11–14 | 4 | Mask `layer_state`, unsigned big-endian |
| 15–18 | 4 | Mask `default_layer_state`, unsigned big-endian |
| 19–22 | 4 | Uptime mili giây 32 bit, big-endian, có rollover |
| 23–31 | 9 | Dự phòng, toàn 0 |

Lớp hiệu lực là bit cao nhất của hai mask OR lại; nếu cả hai là 0 thì lớp 0 theo QMK. Mask ngoài số lớp hoặc số lớp không hợp lệ cho cờ lỗi và chỉ số `FF`; host phải từ chối gửi ảnh. V1 không công bố bộ đếm chuyển lớp hay sự kiện; host poll 100 ms và hỏi lại sau upload để lấy trạng thái mới nhất.

Ví dụ phản hồi lớp giao diện 3: `08006543424c5201030204000000040000000112345678000000000000000000`. Fixture này do chính bộ mã hóa C tạo, được bộ giải mã Rust kiểm tra độc lập.

## Vùng flash và dữ liệu khi nâng cấp

Build dùng cấu hình RP2040/flash 2 MiB từ nguồn hãng; **đã đối chiếu qua BOOTSEL: RP2040 B2, flash 2 MiB; chưa mở vỏ đọc revision PCB**.

Nguồn hãng đặt GIF 100 KiB ngay trước 4 KiB cuối flash, trong khi QMK ở commit này dùng backing store EEPROM 8 KiB cuối. Đã xác nhận các hằng bằng preprocessor với chính cflags của build; có chồng 4 KiB, không chỉ suy đoán từ tên macro.

Keymap `coban_mod/config.h` giữ vị trí/dung lượng EEPROM, lùi vùng GIF 4 KiB. `storage_guard.c` có static assert chống chồng và sai căn sector.

| Vùng, offset tính từ đầu flash 2 MiB | Địa chỉ bắt đầu | Địa chỉ cuối, không gồm |
| --- | --- | --- |
| Ảnh GIF mới, 100 KiB | `0x001E5000` | `0x001FE000` |
| EEPROM backing store, 8 KiB | `0x001FE000` | `0x00200000` |

Phải gửi lại ảnh sau khi nâng cấp vì vị trí ảnh khác. Getter không tự sửa cấu hình, nhưng **VIA có thể reset keymap/macro khi ngày build đổi** (`via_eeprom_is_valid` của nguồn gốc so với `QMK_BUILDDATE`); không hứa nạp UF2 sẽ giữ nội dung người dùng. Sao lưu cấu hình và kiểm tra/khôi phục sau nạp là bước triển khai riêng. Nguồn công khai chưa được chứng minh khớp binary đang dùng; UF2 này không phải bản firmware gốc để khôi phục.

## Dựng lại

Công cụ đã dùng: Arm GNU Toolchain **14.2.Rel1**, GCC 14.2.1; QMK CLI 1.2.0; Python 3.10.19. Toolchain lấy từ [Arm](https://developer.arm.com/-/media/Files/downloads/gnu/14.2.rel1/binrel/arm-gnu-toolchain-14.2.rel1-darwin-arm64-arm-none-eabi.tar.xz), cài trong môi trường build riêng. Timestamp build có thể làm hash bản dựng lại khác.

Tạo checkout riêng và lấy đúng commit/submodule. Nếu dùng sparse checkout phải có builddefs, data, drivers, layouts, lib, platforms, quantum, tmk_core, util và keyboards/coban/pad10a; các tệp gốc ở root vẫn cần giữ.

```sh
git clone --branch cobanpad10a https://github.com/RyanDam/qmk_firmware.git ../qmk-coban
git -C ../qmk-coban checkout f2a3ab0484044e88ae1bb9528764fd4ce6f7f409
git -C ../qmk-coban submodule update --init --recursive
python3 firmware/layer-read/apply.py ../qmk-coban
```

Cài requirements của checkout và QMK CLI trong venv riêng. Đưa venv/bin và thư mục bin của Arm toolchain vào PATH cho lệnh build, rồi chạy tại root QMK:

```sh
make coban/pad10a:coban_mod -j8
```

Lệnh trên chỉ biên dịch. Không chạy target flash hoặc chép UF2 vào thiết bị. Đặt checkout và toolchain ngoài repo ứng dụng để tránh đưa nguồn/dependency build vào Git.

## Kiểm chứng đã làm

- ARM build và link thành công; ELF có symbol `coban_layer_read_reply`, trạng thái lớp lấy từ biến QMK. UF2 đúng magic, mọi block/family RP2040, địa chỉ liên tục trong vùng chương trình và nằm dưới vùng GIF/EEPROM.
- Kiểm thử C với ASan/UBSan: buffer/canary, độ dài thiếu/thừa, lớp 0, vòng TO, nhiều lớp MO/TG, default layer, layer 31, mask vượt giới hạn, count sai, uptime rollover và reserved bytes. Chạy thành công với Clang/Xcode SDK 26.4 trên máy này.
- Rust đọc fixture sinh từ C, từ chối echo firmware gốc, sai header/version/flags/mask/giới hạn. Test qua `Protocol` xác nhận gửi đúng GET có marker. App 0.6.5 có 100 kiểm thử mọi target và Clippy đã qua; kiểm tra thêm legacy flags=1 không nhận SET-size, flags=3 mới dùng handler sửa.
- Guard vùng nhớ được thử thêm bằng chính compiler ARM: ép lại địa chỉ GIF cũ gây lỗi static assertion “GIF must not overlap EEPROM”; cấu hình mới biên dịch thành công.
- Sau khi người dùng yêu cầu nạp: đã verify chương trình trên RP2040 B2/flash 2 MiB, đọc đúng vòng lớp/auto-return qua CBLR, đối chiếu cấu hình và byte GIF flash khớp bản gốc; người dùng xác nhận LCD bình thường. Chưa nghiệm thu Windows, sleep/wake hoặc rollback full-flash thực tế. Trạng thái hiện hành ở [đồng bộ lớp](../../docs/LAYER-SYNC.md).

Kiểm thử C độc lập trên môi trường C11 (có thể bỏ sanitizer nếu compiler không hỗ trợ):

```sh
cc -std=c11 -Wall -Wextra -Werror -pedantic -fsanitize=address,undefined \
  -I firmware/layer-read/overlay firmware/layer-read/overlay/layer_read.c \
  firmware/layer-read/tests/test_layer_read.c -o /tmp/coban-test-layer-read
/tmp/coban-test-layer-read
cargo test --no-default-features device_layer::tests
```

## Đọc lớp sau khi đã nghiệm thu việc nạp

Đóng Coban-mod/Coban Connect để không tranh HID, chạy từ repository ứng dụng:

```sh
cargo run --no-default-features --example read_active_layer -- 120
```

Công cụ chỉ gửi GET. In lớp giao diện 1–4 và mask khi trạng thái thay đổi; báo chưa hỗ trợ nếu firmware vẫn cũ, không đoán lớp. Hãy kiểm tra vòng núm phải, MO/TG nếu sử dụng, timeout tự về lớp mặc định, rút/cắm lại và sleep/wake.

`src/device_layer.rs` và `Protocol::read_active_layer` là adapter cho Coban-mod. **App 0.6.5 dùng getter trực tiếp, bỏ suy luận matrix/mốc lớp, giữ auto-return và chỉ cho bật tự đổi ảnh khi firmware có capability sửa GIF.** Bản 0.6.4 trước đó không đạt thử vật lý và đã bị thay. Firmware 1.1 đã nạp/verify, vượt 30 reload RAM và 10 vòng luân phiên hai GIF; đang kiểm tra tiếp núm cùng UI/LCD. Xem [trạng thái app](../../docs/LAYER-SYNC.md); không coi nghiệm thu firmware riêng là nghiệm thu mọi thao tác gửi ảnh.

## Giấy phép

Phần firmware/overlay/patch theo **GPL-2.0-or-later**, giữ các thông báo bản quyền upstream; xem `COPYING`. Không áp giấy phép MIT của app lên firmware QMK. Repo chứa overlay, kiểm thử và patch 1.1 trên commit upstream cố định để dựng lại; không kèm UF2, full-flash dump hay cấu hình thiết bị cá nhân.

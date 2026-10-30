# Nguồn giao thức Coban Pad 10A

Tóm lược khảo sát và kiểm chứng cục bộ tháng 10/2026, không phải đặc tả của hãng cho mọi revision.

## HID và mô hình

| Trường | Giá trị |
| --- | --- |
| USB product | Coban Pad 10A |
| VID / PID | `0xCB3A` / `0xC10A` |
| Collection cấu hình | Usage Page `0xFF60`, Usage `0x61` |
| Payload input/output | 32 byte, HIDAPI thêm report ID 0 |
| Keyboard collection | Usage Page `0x01`, Usage `0x06` |
| Thiết bị đã đọc | VIA 12, 4 lớp, 16 macro, buffer macro 3783 byte |

Ma trận 3×4: 8 phím và 2 vị trí nhấn núm, thêm 4 chiều xoay mỗi lớp. Dung lượng/giới hạn phải đọc lại từ thiết bị; không lấy kết quả một thiết bị làm mặc định cho mọi firmware.

Đọc/ghi keymap `04/05`, số lớp `11`; macro có count/buffer size/buffer/tên. Custom values GET/SET/SAVE `08/07/09` cho màn hình, RGB, âm thanh, clock/Pomodoro, stats layout và layer timeout. Xem triển khai packet/validation ở `src/protocol.rs`.

Screen mode: `0=clock`, `1=stats`, `2=animation`, `3=layer`, `5=Pomodoro`. Layer timeout tính bằng giây. GIF 160×80 truyền theo chunk 25 byte với offset, giới hạn 102400 byte. Gửi ảnh không atomic. Phân biệt chuỗi legacy với handler sửa GIF theo [CBLR](../firmware/layer-read/README.md).

Các bước ghi phải validate, kiểm tra drift, backup, ghi diff rồi đọc lại. Không công bố đọc ngược ảnh/flash như tính năng backup cấu hình app. Stats layout không đồng nghĩa đã triển khai truyền số đo CPU/RAM.

## Nguồn đã dùng

- [Trang Coban Pad 10A](https://cobanstationery.com/coban-pad-10a).
- [Coban Connect](https://connect.cobanstationery.com/), bản khảo sát v1.5.6.
- [Device features](https://connect.cobanstationery.com/api/device-feature?productId=c10a), SHA-256 khảo sát: `534c951ba7cd6a584d5b270f4abb7ceaee108827d9ce868637fe7c6a7b0f933d`.
- [Device keymap](https://connect.cobanstationery.com/api/device-keymap?productId=c10a), SHA-256: `9f89331d7d171e22bd414ad19fae66692c672e583bf1b06fbc2ac9efa0cb0857`.
- Bundle client `0vkieydkf75wk.js` (transport), SHA-256 `4ab726e171cae24a148a73d53f4126b33d6cce44d2b167c7f610f9f154e01bff`; `2vdt63p5pv901.js` (UI/media), SHA-256 `0a47190244dee415df9f9d3dbfc41304f941c7541a6b06526b1f5369530ec9fa`. Tên/hash là snapshot triển khai, URL có thể hết hạn. Chỉ phân tích để tương thích, không đưa bundle vào mã app.
- [Nguồn QMK nhánh 10A tại commit cố định](https://github.com/RyanDam/qmk_firmware/tree/f2a3ab0484044e88ae1bb9528764fd4ce6f7f409/keyboards/coban/pad10a). Patch giữ USB identity, getter chỉ đọc lớp QMK; tài liệu vùng flash/build/giấy phép nằm trong thư mục firmware.

Ảnh được mã hóa cục bộ, runtime không gọi API xử lý ảnh hãng. USB identity không chứng minh phiên bản firmware; CBLR kiểm tra signature/version/capability để phân biệt hỗ trợ thực tế.

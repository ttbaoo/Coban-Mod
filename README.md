# Coban-mod

Ứng dụng cấu hình **Coban Pad 10A** bằng Rust + Slint, dành cho macOS và Windows. Xử lý ảnh và dữ liệu trên máy, không cần WebView hay tài khoản.

## Sử dụng

Tải gói tại [GitHub Actions → Artifacts](https://github.com/ttbaoo/Coban-Mod/actions) sau khi workflow build thành công, giải nén rồi mở `Coban-mod.app` (macOS Apple Silicon) hoặc `Coban-mod.exe` (Windows x64). Gói macOS từ CI ký ad-hoc, chưa notarize; Windows chưa ký số.

Cắm bàn phím qua USB và đóng Coban Connect/Coban Connect Stats để tránh tranh kết nối. App tự kết nối khi tìm thấy đúng một Pad 10A; nếu có nhiều thiết bị, chọn ở đầu cửa sổ.

- **Coban-mod:** chọn phím, núm hoặc chiều xoay để gán chức năng; chọn lớp 1–4 để chỉnh. Click ô trên màn hình nhỏ để sửa chữ, ảnh, biểu tượng Phosphor hoặc đồng hồ; ô giữa và nền cũng tùy chỉnh được.
- **Macro:** biên tập văn bản ASCII, tổ hợp và khoảng chờ; đặt tên riêng Unicode trên máy. Tên macro gửi thiết bị tối đa 6 ký tự ASCII.
- **Đèn phím, Màn hình, Âm thanh:** cấu hình RGB, ảnh/GIF, đồng hồ, Pomodoro và các thiết lập hãng hỗ trợ. Ảnh được chuyển cục bộ thành GIF 160×80, tối đa 128 màu/100 KiB. Ô Giờ được vẽ/gửi lại mỗi phút khi app chạy, có ghi flash thiết bị.
- **Tự động hóa:** gán F13–F24 để mở website, ứng dụng, tệp, gõ Unicode hoặc chuỗi phím. Nhận phím tự bật sau khi kết nối; quyền Input Monitoring/Accessibility của macOS vẫn cần được cấp khi hệ thống yêu cầu. Website nhận cả `youtube.com` và URL đầy đủ.
- **Sao lưu & khôi phục:** xuất/khôi phục keymap, encoder, macro và thiết lập; bản sao cấu hình không chứa firmware hoặc ảnh cũ trong flash.
- **Cài đặt:** khởi động cùng hệ điều hành, dịch vụ nhận phím, giao diện và ngôn ngữ.

Chỉnh sửa hoàn tất sẽ tự kiểm tra, sao lưu, ghi và đọc lại. Chỉ báo thành công sau khi dữ liệu khớp; có Hoàn tác cấu hình. Chọn ô hoặc chọn lớp để xem không tự ghi binding. Mất kết nối/lỗi ghi cần đọc lại hoặc thử gửi lại, không tự phát lại thay đổi cũ.

macOS: đóng cửa sổ giữ app trên menubar; **Thoát Coban-mod** dừng cả app và agent. Windows: đóng cửa sổ dừng cả hai. Khởi động cùng hệ điều hành mở app ở chế độ nền; macOS cần phiên bản 13+ cho Login Item, app vẫn hỗ trợ macOS 12.

## Đồng bộ ảnh theo lớp

Các chức năng cấu hình cơ bản dùng được với firmware gốc. **Theo lớp bàn phím** đọc lớp thật qua CBLR; **Bật tự đổi ảnh** chỉ khả dụng với firmware bổ sung **1.1**, có bản sửa reload GIF (`flags=3`). App không tự nạp firmware.

Mỗi lớp có bố cục ảnh riêng; lớp chưa chỉnh kế thừa ảnh cũ nên có thể trông giống nhau. Chọn lớp bằng tay giữ lớp đang chỉnh đến lần đổi lớp thật kế tiếp. Xem [cơ chế và giới hạn kiểm chứng](docs/LAYER-SYNC.md) và [nguồn/build firmware](firmware/layer-read/README.md).

## Build từ mã nguồn

Cần Rust stable **1.92+**, Python **3.11+**; Xcode Command Line Tools trên macOS hoặc Visual Studio Build Tools (C++, Windows SDK/`rc.exe`) trên Windows.

```sh
cargo build --locked --release --bin mk
```

macOS cần thêm helper trước khi đóng gói:

```sh
cargo build --locked --release --no-default-features --bin mk-agent
python3 scripts/package.py --mac
```

Windows, sau khi build trên Windows:

```sh
python scripts/package.py --windows
```

Kết quả nằm trong `dist/`. Hướng dẫn kiểm thử, cross-build, ký gói và cấu trúc mã ở [DEVELOPMENT.md](docs/DEVELOPMENT.md). CI tự kiểm tra và đóng gói trên macOS/Windows khi push hoặc mở pull request.

## Dữ liệu và giới hạn

Dữ liệu người dùng nằm trong thư mục ứng dụng `MK` của hệ điều hành. Bố cục ảnh/tên riêng/hành động trên máy độc lập với bản sao cấu hình thiết bị. Giữ `MK`, biến `MK_*`, định dạng `.mk.json`, bundle ID `vn.baotruong.mk` / `vn.baotruong.mk.agent` và helper `MK Agent.app` để tương thích bản cũ; tên ứng dụng chính thức là **Coban-mod**.

Bản mã nguồn hiện tại: **0.6.5**. Kiểm thử tự động dùng transport giả, không ghi bàn phím thật. Windows runtime/phần cứng, reconnect và sleep/wake thực tế chưa nghiệm thu; tự đổi ảnh cùng UI/LCD còn chờ xác nhận đầy đủ. Chưa có truyền số đo CPU/RAM, import backup hãng, updater hoặc ghi macro tự động từ phím. English mới bao phủ điều khiển chính; giao diện/ngôn ngữ chưa lưu qua lần chạy.

## Giấy phép

Ứng dụng: [MIT](LICENSE). Firmware bổ sung QMK: [GPL-2.0-or-later](firmware/layer-read/COPYING). Phosphor Icons kèm giấy phép tại [assets/phosphor](assets/phosphor/LICENSE); Slint và các dependency có giấy phép riêng. [Nguồn giao thức thiết bị](docs/DEVICE-PROTOCOL.md).

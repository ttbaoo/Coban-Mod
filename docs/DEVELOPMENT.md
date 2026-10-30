# Phát triển Coban-mod

## Cấu trúc

| Thư mục | Nội dung |
| --- | --- |
| `src/` | Rust core, transport HID, broker/IPC, host actions và app |
| `ui/` | Giao diện Slint và component dùng chung |
| `tests/` | Kiểm thử lõi, vòng đời agent, gửi ảnh |
| `assets/` | Icon ứng dụng/UI, keycode và font Phosphor kèm giấy phép |
| `scripts/` | Đóng gói và sinh tài nguyên |
| `firmware/layer-read/` | Getter CBLR, patch 1.1, test C và hướng dẫn build |
| `examples/` | Chẩn đoán lớp và công cụ chuẩn bị/khôi phục sau nạp |

Đọc [DESIGN.md](../DESIGN.md) trước khi thay đổi hành vi. Binary nội bộ `mk` / `mk-agent`, crate `mk`; tên app/gói là Coban-mod.

## Kiểm tra

Dùng Rust 1.92+, Python 3.11+ và toolchain native như README. Kiểm thử dùng thư mục dữ liệu tạm riêng và transport giả; không cần kết nối bàn phím.

```sh
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
```

`MK_DATA_DIR` có thể trỏ tới thư mục riêng khi QA. Binary `mk-ui-qa` chỉ dành cho kiểm tra UI với `--features internal-ui-qa`, không đóng gói vào sản phẩm. Kiểm thử C của firmware được ghi trong [hướng dẫn firmware](../firmware/layer-read/README.md).

## Đóng gói

Build UI release và helper như [README](../README.md). macOS dùng `scripts/package.py --mac`; Windows dùng `scripts/package.py --windows`. `--out-dir PATH` chọn đầu ra khác.

Ký macOS qua `--sign-identity`, `MK_CODESIGN_IDENTITY` hoặc `.tools/package-local.json` (khóa `macos_signing_identity`); helper ký trước app. Không cấu hình thì ký ad-hoc. Phân phối cần tự thực hiện quy trình ký/notarize phù hợp; script không đăng ký dịch vụ hoặc thay quyền OS. Gói cũ ở cùng đầu ra được giữ thành `Coban-mod.previous.app`.

Cross-build Windows từ macOS cần mingw-w64 và Rust target GNU:

```sh
rustup target add x86_64-pc-windows-gnu
cargo build --locked --release --target x86_64-pc-windows-gnu --bin mk
python3 scripts/package.py --windows
```

Build native Windows dùng MSVC và resource compiler `rc.exe`. Workflow [build.yml](../.github/workflows/build.yml) kiểm tra/đóng gói cả macOS lẫn Windows; artifact CI không thay nghiệm thu runtime/phần cứng.

## Tài nguyên và công cụ

- `scripts/make_icons.py`: sinh PNG iconset macOS và ICO Windows bằng Python chuẩn.
- `scripts/make_ui_icons.py`: sinh SVG cho UI; `assets/mk.svg` là icon vector ứng dụng.
- `scripts/make_phosphor.py PATH_TO_PACKAGE`: chép font và sinh bảng mã từ `phosphoricons_flutter` 1.0.0. Tài nguyên đã commit nên build bình thường không cần package Flutter.
- `examples/read_active_layer.rs`: GET CBLR khi app/agent đã dừng; không ghi thiết bị.
- `examples/watch_layer_sync.rs`: hỏi trạng thái IPC khi app chạy, không tranh HID.
- `examples/enter_bootloader.rs`, `post_flash_restore.rs`: công cụ thủ công cho người phát triển firmware; yêu cầu backup/định danh khớp. Không chạy trong kiểm thử tự động.

CLI app có `--list`, `--inspect`, `--backup FILE`, `--check-backup FILE`, `--agent-status`, `--agent-stop`. Dữ liệu cá nhân, backup/flash dump, cấu hình ký, cache và gói build nằm ngoài Git; giữ bản phục hồi ngoài repo trước khi dọn các thư mục này.

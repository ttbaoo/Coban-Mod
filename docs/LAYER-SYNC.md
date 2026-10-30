# Đồng bộ lớp và ảnh

## Cách hoạt động

App 0.6.5 đọc `GET 08 00 65` có chữ ký CBLR từ firmware bổ sung; không suy luận từ matrix hoặc lớp đang chỉnh. Giao thức chi tiết và nguồn firmware ở [firmware/layer-read](../firmware/layer-read/README.md).

- **Theo lớp bàn phím** tải chức năng/bố cục theo lớp thực tế. Chọn tay một lớp giữ lớp đang chỉnh đến lần chuyển lớp thật kế tiếp; tắt công tắc nếu muốn giữ lớp đang chỉnh lâu hơn.
- **Bật tự đổi ảnh** đối chiếu và sao lưu trước khi chọn Ảnh/GIF, tắt overlay lớp gốc. Giữ binding, macro, encoder và timeout tự về lớp mặc định. Tắt chỉ dừng gửi ảnh tự động.
- Tùy chọn lưu riêng theo identity tại `layer-image-sync-<identity-hash>.json`. Mở lại/reconnect phải đọc lớp thật và chỉ tiếp tục nếu preference bật, thiết bị đang ở chế độ Ảnh/GIF.
- `screen-layout-layer-1.json` đến `screen-layout-layer-4.json` giữ bố cục từng lớp; lớp chưa có bố cục kế thừa `screen-layout.json`. Nhiều lớp có thể trông giống nhau cho tới khi chỉnh ảnh riêng.
- Broker giữ độc quyền USB; xen GET giữa các chunk gửi ảnh, hoàn thành ảnh đang gửi rồi xử lý lớp mới nhất. Có độ trễ truyền ảnh. Lỗi dừng tự gửi; không replay binding/macro.

## Tương thích và bản sửa

Firmware gốc hoặc CBLR cũ (`flags=1`) vẫn dùng cấu hình/gửi ảnh thủ công qua chuỗi legacy; tự đổi ảnh bị khóa. Chỉ firmware **1.1 (`flags=3`)** nhận SET-size dùng khi tự đổi ảnh.

Bản thử 0.6.4 gặp timeout cả GET lớp lẫn GET cấu hình sau khi gửi ảnh. Nguồn firmware hãng tạo hai decoder khi reload GIF ở mode GIF, có thể cạn heap. Firmware 1.1 xóa view cũ và chỉ reload background; capability bit 1 nhận diện handler đã sửa. App không gửi SET-size vào handler cũ.

## Phạm vi đã kiểm chứng

- C/Rust kiểm tra payload, mask, giới hạn, echo/sai phiên bản và capability; broker kiểm tra lớp thật khác lớp đang chỉnh, đổi lớp giữa upload, không đổi keymap/macro/timeout.
- Firmware 1.1 đã build/nạp/verify trên RP2040 B2, flash 2 MiB. Getter ghi được nhiều vòng 1→2→3→4→1 và auto-return khoảng 60 giây. Cấu hình/ảnh sau nạp đã đối chiếu; người dùng xác nhận LCD bình thường.
- 30 reload RAM và 10 vòng luân phiên hai GIF vẫn trả GET; không SAVE flash trong phép thử này. Đây là kiểm tra giao thức/khả năng đáp ứng, không xác minh pixel.
- Bản 0.6.5 đã gửi ảnh theo nhiều vòng lớp qua IPC; lượt đầu người dùng vẫn chưa thấy UI/LCD chuyển rõ vì các lớp kế thừa cùng ảnh và thao tác chọn tay. Đã sửa giữ chọn tay đến lần đổi lớp kế tiếp và tạo ảnh riêng cho thử nghiệm.
- **Chưa nghiệm thu đầy đủ tự đổi ảnh UI/LCD**, reconnect, sleep/wake hoặc Windows runtime. ACK và readback mode không chứng minh pixel LCD đã đổi.

Theo dõi trạng thái khi app đang chạy:

```sh
cargo run --no-default-features --example watch_layer_sync -- 120
```

Đọc CBLR trực tiếp sau khi đã đóng app/agent:

```sh
cargo run --no-default-features --example read_active_layer -- 120
```

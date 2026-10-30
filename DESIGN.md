# Coban-mod — quyết định hiện hành

Tài liệu giữ những lựa chọn người dùng đã xác nhận và giới hạn triển khai đang áp dụng. Cập nhật 30/10/2026; phiên bản nguồn 0.6.5. Các biên bản từng bản thử đã được dọn khỏi repo public.

## Phạm vi đã xác nhận

- Rust/HID + Slint, macOS và Windows; cấu hình Coban Pad 10A qua giao thức hãng. Tên app **Coban-mod** (m thường), gói `Coban-mod.app` / `Coban-mod.exe`; repo `ttbaoo/Coban-Mod`.
- Giữ USB identity `Coban Pad 10A`, VID/PID `CB3A:C10A`, descriptor, bundle ID, helper `MK Agent.app`, dữ liệu `MK`, biến `MK_*` và định dạng backup để tương thích. Không đổi firmware/USB chỉ để đổi tên app.
- Người dùng cho phép bổ sung firmware getter lớp ngày 29/10, gồm bản sửa reload GIF và chồng vùng flash cần thiết; không phải kế hoạch viết lại toàn bộ firmware. Nạp thiết bị là thao tác riêng, có backup toàn flash và verify trước/sau; app không tự nạp firmware.
- Đã xác nhận dọn repo để công khai và bỏ lịch sử commit cũ ngày 30/10. Giữ nguồn hiện tại, công cụ còn dùng, kiểm thử và giấy phép; bản sao riêng ngoài repo giữ dữ liệu phát triển/phục hồi.

## Tương tác và bảo toàn dữ liệu

- Không có chế độ mô phỏng hoặc nút Áp dụng trong sản phẩm. Đọc thật khi kết nối; thay đổi hoàn tất → validate → đối chiếu baseline → backup → ghi diff tuần tự → đọc lại/xác minh. Có Hoàn tác cấu hình; không tự rollback mù.
- Slider ghi giá trị chốt; trường chữ chốt Enter/rời ô. Chọn vị trí/lớp để xem không ghi binding. Không tự phát lại thao tác sau lỗi hoặc reconnect; khóa chỉnh khi chưa kết nối/đọc lại.
- UI không sở hữu HID hay thực thi hành động trực tiếp. UI → IPC có version/instance/request ID → agent theo phiên người dùng → một broker sở hữu thiết bị và kho dữ liệu. Worker riêng xử lý hành động; không chặn UI bằng USB/file.
- Định danh đúng path/collection, serial hoặc physical port/container khi thiếu serial; không gộp theo VID/PID. Đọc phản hồi kiểm tra header/length; timeout không tự ghi lại. Không chạy đồng thời writer hãng.
- Backup riêng tư có schema/identity/checksum, ghi atomic; chứa keymap, encoder, macro buffer/tên và custom settings. Không hứa sao lưu firmware/ảnh flash qua backup cấu hình.

## UI, ảnh và lớp

- Trang Coban-mod đặt đầu thanh bên, sơ đồ 8 phím và 2 núm nhấn/xoay; chọn vị trí để chỉnh Chức năng hoặc Ảnh hiển thị. Các trang Macro, Đèn phím, Màn hình, Âm thanh, Sao lưu, Tự động hóa, Cài đặt dùng component/token đã dựng trong `ui/main.slint`.
- Ảnh là GIF ghép toàn màn hình 160×80; 8 ô phím, 2 núm và ô giữa, cộng nền. Sửa trực tiếp → tự dựng/lưu/gửi. Preview sắc nét 640×320 hoặc xem đúng GIF 160×80; viền chọn ô không xuất vào ảnh.
- Biểu tượng Phosphor (MIT) nhúng ba nét Thường/Đậm/Đặc, thống nhất hai OS. Di chuyển tên SF Symbols cũ khi đọc; không đóng gói bộ Apple.
- Ô giữa có Chữ/Ảnh/Biểu tượng/Trống/Giờ. Giờ có kiểu Thường/Đậm/LED, định dạng giờ/ngày. **Người dùng chọn vẽ giờ vào ảnh và gửi lại mỗi phút**, chấp nhận ghi flash và chỉ cập nhật khi app chạy. Không đổi binding/keymap khi cập nhật giờ.
- **Người dùng yêu cầu** nhấn núm phải chuyển lớp theo vòng, UI và LCD cùng theo lớp. Không suy ra lớp thật từ tab đang chỉnh hoặc matrix. App đọc CBLR GET; tự về lớp mặc định giữ nguyên.
- Theo lớp bàn phím điều khiển lớp đang xem; chọn tay giữ đến lần đổi lớp thật kế tiếp. Tự đổi ảnh là lựa chọn riêng theo identity; chỉ bật với capability sửa GIF của firmware 1.1 (`flags=3`). Lỗi dừng tự gửi, không đoán lớp.
- Lớp chưa có ảnh riêng kế thừa bố cục cũ. Giữ tệp cũ; không lấy ACK/readback mode làm bằng chứng pixel LCD. Xem [đồng bộ lớp](docs/LAYER-SYNC.md).

## Host actions và vòng đời

- Mở website, ứng dụng và tệp là ba loại riêng; thêm gõ Unicode/chuỗi phím. URL thiếu giao thức dùng HTTPS. Tên riêng macro/F13–F24 nhận Unicode tối đa 80 ký tự, độc lập tên macro ASCII 6 ký tự trong firmware và nhãn LCD.
- App kết nối/đọc cấu hình xong tự bật nhận phím mỗi phiên/kết nối; chỉ nghe bàn phím được chọn. Quyền macOS và giới hạn quyền Windows vẫn áp dụng. Không sửa TCC hoặc tự bỏ kiểm tra quyền.
- App chạy thì agent chạy; Thoát dừng cả hai. Agent tự dừng sau 30 giây không nhận yêu cầu UI. Đóng cửa sổ macOS giữ app/agent trên menubar; Windows đóng cửa sổ thoát cả hai. Đợi thao tác lưu hoàn tất.
- Khởi động cùng OS mặc định tắt; mở app `--background`. macOS SMAppService cần 13+, giữ minimum app 12. Windows user startup mở app, app mở agent.

## Tình trạng kiểm chứng

- Kiểm thử lõi, IPC, UI và broker dùng transport giả. Không ghi phím/macro thật chỉ để thử UI. Cần kiểm chứng phần cứng/OS độc lập với cross-build.
- Firmware 1.1 đã nạp/verify trên RP2040 B2, flash 2 MiB; đọc được vòng lớp/auto-return, 40 lượt reload GIF trong RAM còn đáp ứng. Cấu hình/ảnh sau triển khai đã được đối chiếu, người dùng xác nhận LCD bình thường.
- 0.6.4 không đạt thực tế, bị thay bằng 0.6.5/firmware 1.1. Tự đổi ảnh 0.6.5 cùng layout riêng vẫn chờ nghiệm thu đầy đủ UI/LCD. Windows runtime, reconnect/sleep-wake thực tế còn mở.
- Chưa có truyền CPU/RAM, import backup hãng, updater hoặc ghi macro tự động; English chưa đầy đủ, theme/ngôn ngữ chưa lưu. Không trình bày các mục này như đã hoàn thành.

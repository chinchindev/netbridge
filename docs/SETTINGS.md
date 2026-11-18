# Setting cá nhân, sao lưu và khôi phục

Cài mới bắt đầu với danh sách quy tắc và đích kiểm tra trống. Người dùng tự thêm
quy tắc trong **Quy tắc**, chọn adapter trong **Kết nối** và khai báo tối đa 4 đích
TCP trong **Cài đặt → Đích kiểm tra kết nối**. Đích kiểm tra không nhận URL,
wildcard hoặc CIDR; nhập hostname/IPv4 và cổng riêng. Quy tắc phân luồng vẫn hỗ trợ CIDR/wildcard.

Nút kiểm tra kết nối thử trực tiếp các đường mạng theo quy tắc đã lưu; kết quả đạt
không có nghĩa proxy Windows đã bật hoặc chứng chỉ TLS đã được xác minh.

## Sao lưu

Chọn **Cài đặt → Sao lưu setting**, chọn nơi lưu file JSON. Sao lưu lấy cấu hình
đã lưu; hãy lưu hoặc hủy bản nháp trước. File bao gồm:

- Cổng proxy, quy tắc, adapter LAN/USB, đường dẫn ADB.
- Đích kiểm tra kết nối và đơn vị tốc độ.

Không bao gồm phiên proxy, snapshot khôi phục Windows, log, danh sách thiết bị,
khóa ký cập nhật hoặc kết quả đo mạng. File có thể chứa địa chỉ riêng và đường dẫn
trên máy; giữ file riêng tư, không commit hoặc upload công khai.

## Khôi phục

1. Tắt phiên phân luồng và chờ các tác vụ hoàn tất.
2. Chọn **Khôi phục từ file**, xem thông tin và chọn lại adapter LAN/USB nếu chuyển máy.
   Mapping trong file được giữ mặc định; adapter đang ngắt vẫn có lựa chọn giữ lại.
   Có thể chọn **Dùng ADB tích hợp trên máy này** thay cho đường dẫn ADB từ máy cũ.
3. Chọn **Xác nhận khôi phục**. App giữ bản cấu hình trước tại
   `%LOCALAPPDATA%\NetBridge\settings-backups\<id>\` trước khi ghi.
4. Kiểm tra lại adapter khi chuyển sang máy khác, rồi chủ động bật phân luồng.

Thay đổi trong hộp thoại chỉ được áp dụng sau khi xác nhận. Hủy hộp thoại giữ nguyên
setting đang dùng. Nếu LAN và USB chọn cùng một adapter, app sẽ cảnh báo rằng hai vai trò
không tạo ra hai đường mạng khác nhau.

Restore không bật proxy, không thay route/metric Windows. File sai schema/version,
lớn hơn 512 KB hoặc cấu hình không hợp lệ bị từ chối trước khi thay setting.
Nếu cấu hình bị thay đổi sau khi xem trước, chọn lại file để xác nhận trên bản hiện tại.
Nếu ghi config thất bại sau khi ghi đơn vị, app phục hồi preference trước đó;
thư mục recovery được giữ để xử lý nếu filesystem không cho phục hồi.

## Tương thích

Config từ phiên bản trước vẫn được đọc và giữ nguyên quy tắc; `check_targets` chưa có
được hiểu là danh sách trống. Sao lưu dùng `format: netbridge-settings`, `version: 1`.
Đây là schema riêng của backup, không phải version ứng dụng. File config.json cũ
không phải file backup hoàn chỉnh vì chưa chứa preference và thông tin schema.

Các địa chỉ xuất hiện trong browser demo/test là ví dụ thuộc example.test,
example.org hoặc dải IP tài liệu; không phải preset của bản cài mới.

## Khi cấu hình bị lỗi

Từ 0.5.0, file cấu hình được ghi qua file tạm đã flush rồi mới thay thế file chính.
Bản `config.json.bak` chỉ được cập nhật từ cấu hình hợp lệ. Khi file chính hỏng,
Desktop vẫn mở màn hình phục hồi và khóa thao tác mạng; không tự dùng một profile
trống để bật proxy.

Chọn **Phục hồi bản lưu gần nhất** và xác nhận, hoặc nhập một file backup setting.
File hỏng được giữ trong `settings-backups` trước khi thay thế. Nếu file đổi sau lúc
hiển thị cảnh báo, app từ chối phục hồi trên revision cũ. Lỗi quyền truy cập hoặc
filesystem phải được xử lý trước khi app có thể ghi lại dữ liệu.

Ghi nguyên tử bảo vệ từng file. Config và preference vẫn là hai file riêng; không
coi chúng là một transaction chống mọi trường hợp mất điện giữa hai lần ghi.

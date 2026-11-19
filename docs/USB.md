# Kết nối USB với điện thoại Android

1. Bật USB debugging trên điện thoại và chấp nhận yêu cầu cấp quyền ADB.
2. Trong NetBridge, chọn đúng thiết bị USB có trạng thái `device`.
3. Bật **USB tethering / Chia sẻ kết nối USB** trong Cài đặt điện thoại khi cần
   chia sẻ Internet. Bật giao diện RNDIS qua ADB không luôn tự bật dịch vụ tethering.
4. Làm mới danh sách adapter, chọn card USB của điện thoại và lưu cấu hình.
   Khi đổi từ thiết bị khác sang điện thoại mới, GUID adapter có thể khác.
5. Chỉ bật phân luồng sau khi adapter USB đã có IPv4 và đường mạng cần dùng.

NetBridge kiểm tra khả năng của `svc usb` trước khi gửi lệnh. Android cũ, như
SM-G930U chạy Android 8.0, dùng `setFunction`; Android mới dùng `setFunctions`.
Help có thể được trả về trên stderr dù lệnh thành công. Cả luồng bật/ngắt và
phục hồi chức năng USB đều dùng biến thể mà thiết bị hỗ trợ.

ADB có thể mất kết nối tạm thời khi điện thoại đổi chức năng USB. Kết quả được
xác nhận lại bằng `sys.usb.state`, không chỉ dựa vào exit code của lệnh đổi chế độ.
Trên Android cũ, thao tác bật RNDIS cho phép USB data; khi hoàn tác về chức năng
cũ, app không tự mở quyền truyền dữ liệu MTP/PTP.

`rndis,adb` xác nhận giao diện USB Ethernet và ADB đã bật; nó chưa chứng minh
điện thoại đã cấp DHCP/gateway hoặc có Internet. Cần kiểm tra riêng adapter và
trạng thái chia sẻ kết nối trên điện thoại.

Thao tác cấu hình qua ADB không cần Administrator trên Windows. Nếu Windows báo
thiếu hoặc lỗi driver, người dùng không có quyền admin cần IT hỗ trợ phần driver;
không dùng việc nâng quyền làm bước mặc định khi xử lý lỗi ADB.

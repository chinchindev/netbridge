# Phạm vi phân luồng và ứng dụng dùng proxy

NetBridge định tuyến các kết nối đi qua proxy của nó. Bật phân luồng cấu hình proxy
Windows cho ứng dụng tuân theo thiết lập đó; ứng dụng kết nối trực tiếp vẫn dùng route Windows.

Địa chỉ hiện tại được hiển thị trong Cài đặt → Proxy cho ứng dụng. Mặc định là
`http://127.0.0.1:17890`; dùng đúng cổng của profile nếu đã đổi.

Trình duyệt thường sử dụng proxy Windows. Git/Fork và công cụ CLI có thể cần proxy riêng.
Ví dụ, chạy trong repository Git cần đi qua NetBridge:

```powershell
git config --local http.proxy http://127.0.0.1:17890
```

Thiết lập này áp dụng cho HTTP/HTTPS của repository đó, không áp dụng cho SSH.
Khi không dùng NetBridge cho repository nữa, hoàn tác riêng khóa vừa thêm:

```powershell
git config --local --unset-all http.proxy
```

Giữ phân luồng bật khi ứng dụng đang trỏ tới proxy NetBridge. Nếu proxy dừng hoặc đổi cổng,
ứng dụng có cấu hình riêng cần được cập nhật tương ứng. Không tắt kiểm tra chứng chỉ TLS
để xử lý một lỗi kết nối.

Bảng chẩn đoán so sánh TCP trực tiếp của Windows với đường theo quy tắc; không có nghĩa
mọi ứng dụng trên máy đang đi qua proxy, và TCP đạt không chứng minh TLS hoặc đăng nhập đạt.

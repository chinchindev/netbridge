# NetBridge Desktop

Ứng dụng Windows x64 quản lý phân luồng theo đích qua LAN và USB Android.
Giao diện Angular/Tauri, lõi Rust, ADB tích hợp, kiểm tra TCP và đo tốc độ mạng.

## Sử dụng

Dùng bộ cài NSIS từ [GitHub Releases](https://github.com/chinchindev/netbridge/releases).
EXE rời cần Microsoft Edge WebView2 Runtime có sẵn. Bộ cài phát hành chứa runtime offline.

- Chọn adapter LAN/USB, kiểm tra quy tắc rồi bật phân luồng.
- Sửa cấu hình chỉ tạo draft; chọn Lưu thay đổi để áp dụng cho kết nối mới.
- Đóng cửa sổ khi có phiên/job sẽ ẩn xuống tray. Tắt phiên/Thoát xử lý phục hồi proxy.
- Trong Cài đặt, chọn Kiểm tra cập nhật NetBridge rồi xác nhận tải/cài khi có bản mới.
  Tải và xác minh chữ ký hoàn tất trước khi tắt phiên để cài đặt.

Preview không thay cấu hình hoặc mạng:

```powershell
.\NetBridge.exe --preview
```

## Phát triển trên Windows

Cần Node 24, Rust MSVC 1.95.0, Visual Studio Build Tools với workload C++ và Windows SDK.

```powershell
cd desktop
npm ci
npm run build
cargo test --locked --manifest-path src-tauri/Cargo.toml
npm run test:e2e
npm run test:production
```

Frontend browser dùng dữ liệu mô phỏng có banner; không phải số đo mạng thật.
Bộ công cụ cục bộ riêng của workspace có thể dùng `desktop/build-desktop.ps1`.
GitHub Actions dùng toolchain cài sẵn/thiết lập trên runner Windows, không tải thư mục
`.build-tools` của máy phát triển lên repository.

## CI, phiên bản và cập nhật

- `.github/workflows/windows.yml`: kiểm thử, Clippy, frontend và build EXE trên push/PR.
- `.github/workflows/release.yml`: tag `vX.Y.Z` tạo installer có chữ ký cập nhật,
  `latest.json`, checksum và GitHub Release.
- Hướng dẫn cấu hình repository/secrets, tăng version và phát hành: [RELEASING.md](docs/RELEASING.md).

Nguồn core Win32 vẫn được giữ. Các bản RC hiện có chưa nghiệm thu toàn bộ lifecycle
live hoặc cài offline trên máy sạch; kiểm thử unit/browser không thay thế các ca đó.
Chữ ký updater dùng xác minh gói tải về, không phải chứng chỉ Authenticode Windows.

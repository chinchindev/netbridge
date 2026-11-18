import {test,expect} from '@playwright/test';
test('combined USB action reaches an active routing session',async({page})=>{
 await page.goto('/');await page.getByRole('button',{name:'Kết nối',exact:true}).click();
 await page.getByRole('button',{name:'Kết nối USB & phân luồng',exact:true}).click();
 await page.getByRole('button',{name:'Tổng quan',exact:true}).click();await expect(page.locator('.session-copy h2')).toHaveText('Đang phân luồng');
});
test('combined action requires a saved draft and reports failed routing',async({page})=>{
 await page.goto('/?scenario=usb-connect-failure');await page.getByRole('button',{name:'Kết nối',exact:true}).click();
 const combined=page.getByRole('button',{name:'Kết nối USB & phân luồng',exact:true});await page.locator('#lan').click();await page.getByRole('option',{name:'Ethernet 3 · 198.51.100.10',exact:true}).click();await expect(combined).toBeDisabled();
 await page.getByRole('button',{name:'Hủy thay đổi',exact:true}).click();await combined.click();
 await expect(page.getByRole('alert')).toContainText('Không bật được phân luồng');
 await page.getByRole('button',{name:'Tổng quan',exact:true}).click();await expect(page.locator('.session-copy h2')).not.toHaveText('Đang phân luồng');
});
test('corrupt configuration requires confirmation before recovery',async({page})=>{
 await page.goto('/?scenario=config-error');await expect(page.getByRole('alert')).toContainText('Cấu hình cần phục hồi');await expect(page.getByRole('button',{name:'Bật phân luồng',exact:true})).toBeDisabled();
 await page.getByRole('button',{name:'Phục hồi bản lưu gần nhất',exact:true}).click();await expect(page.getByRole('dialog',{name:'Phục hồi cấu hình',exact:true})).toBeVisible();
 await page.getByRole('button',{name:'Để nguyên cấu hình',exact:true}).click();await expect(page.getByRole('alert')).toContainText('Cấu hình cần phục hồi');
 await page.getByRole('button',{name:'Phục hồi bản lưu gần nhất',exact:true}).click();await page.getByRole('button',{name:'Xác nhận phục hồi',exact:true}).click();await expect(page.getByText('Cấu hình cần phục hồi', {exact:true})).toHaveCount(0);
});
test('missing recovery backup still permits importing a valid settings file',async({page})=>{
 await page.goto('/?scenario=config-error-no-backup');await expect(page.getByRole('button',{name:'Phục hồi bản lưu gần nhất',exact:true})).toBeDisabled();
 await page.getByRole('button',{name:'Cài đặt',exact:true}).click();await expect(page.getByRole('button',{name:'Khôi phục từ file',exact:true})).toBeEnabled();await expect(page.getByRole('button',{name:'Sao lưu setting',exact:true})).toBeDisabled();
});
test('route diagnostics distinguish Windows failure from policy success',async({page})=>{
 await page.goto('/?scenario=windows-route-failure');await page.getByRole('button',{name:'Kiểm tra kết nối',exact:true}).click();
 const comparison=page.locator('.target-comparison').first();await expect(comparison).toContainText('Windows trực tiếp');await expect(comparison).toContainText('Không kết nối');await expect(comparison).toContainText('Theo quy tắc');await expect(comparison).toContainText('TCP đạt');await expect(comparison).toContainText('dự kiến');
});
test('a corrected external file can be reloaded while the app is in recovery',async({page})=>{
 await page.goto('/?scenario=config-error-external-fix');const reload=page.getByRole('button',{name:'Tải cấu hình mới',exact:true});await expect(reload).toBeEnabled();await reload.click();await expect(page.getByText('Cấu hình cần phục hồi',{exact:true})).toHaveCount(0);
});

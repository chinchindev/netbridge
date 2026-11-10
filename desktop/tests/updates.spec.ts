import {test,expect} from '@playwright/test';
test('application update requires confirmation and shows a separate progress state',async({page})=>{
 await page.goto('/');await page.getByRole('button',{name:'Cài đặt',exact:true}).click();
 await page.getByRole('button',{name:'Kiểm tra cập nhật NetBridge',exact:true}).click();
 await expect(page.getByText('Có phiên bản 0.4.0')).toBeVisible();
 await page.getByRole('button',{name:'Tải và cài bản mới',exact:true}).click();
 await expect(page.getByRole('dialog',{name:'Cập nhật NetBridge',exact:true})).toBeVisible();
 await page.getByRole('button',{name:'Để sau',exact:true}).click();
 await expect(page.getByRole('dialog')).toHaveCount(0);
 await page.getByRole('button',{name:'Tải và cài bản mới',exact:true}).click();
 await page.getByRole('button',{name:'Xác nhận cập nhật',exact:true}).click();
 await expect(page.getByText('Đã mô phỏng cập nhật. Không cài đặt trên máy.')).toBeVisible();
});
test('application update reports network failure and allows retry',async({page})=>{
 await page.goto('/?scenario=update-error');await page.getByRole('button',{name:'Cài đặt',exact:true}).click();
 await page.getByRole('button',{name:'Kiểm tra cập nhật NetBridge',exact:true}).click();
 await expect(page.locator('.app-update')).toContainText('Không kết nối được máy chủ cập nhật');
 await expect(page.getByRole('button',{name:'Kiểm tra cập nhật NetBridge',exact:true})).toBeEnabled();
});

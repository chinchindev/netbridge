import {test,expect} from '@playwright/test';
const backup={format:'netbridge-settings',version:1,config:{port:18001,lan_id:'demo-lan',usb_id:'demo-usb',adb_path:'auto',rules:[{pattern:'restored.example.test',route:'lan'}],check_targets:[{name:'Restored portal',host:'restored.example.test',port:8443}]},preferences:{unit:'MiB/s'}};
test('fresh settings have no predefined targets and a target requires saving',async({page})=>{
 await page.goto('/?scenario=empty-settings');await expect(page.getByText('Chưa có đích kiểm tra')).toBeVisible();
 await page.getByRole('button',{name:'Cài đặt',exact:true}).click();
 await page.locator('#check-name').fill('Personal portal');await page.locator('#check-host').fill('personal.example.test');await page.locator('#check-port').fill('8443');
 await page.getByRole('button',{name:'Thêm đích',exact:true}).click();await expect(page.getByRole('button',{name:'Sao lưu setting',exact:true})).toBeDisabled();
 await page.getByRole('button',{name:'Tổng quan',exact:true}).click();await expect(page.getByText('Chưa có đích kiểm tra')).toBeVisible();
 await page.getByRole('button',{name:'Lưu thay đổi',exact:true}).click();await expect(page.getByText('personal.example.test:8443')).toBeVisible();
});
test('backup downloads saved settings with no runtime logs',async({page})=>{
 await page.goto('/');await page.getByRole('button',{name:'Cài đặt',exact:true}).click();
 const download=page.waitForEvent('download');await page.getByRole('button',{name:'Sao lưu setting',exact:true}).click();
 const file=await download;const stream=await file.createReadStream();const chunks:Buffer[]=[];for await(const chunk of stream!)chunks.push(chunk);
 const json=JSON.parse(Buffer.concat(chunks).toString('utf8'));expect(json.format).toBe('netbridge-settings');expect(json.version).toBe(1);expect(json.config.rules.length).toBe(4);expect(json).not.toHaveProperty('logs');expect(json).not.toHaveProperty('session');
});
test('restore previews, can cancel, then applies rules and units',async({page})=>{
 await page.goto('/');await page.getByRole('button',{name:'Cài đặt',exact:true}).click();
 async function choose(){const chooser=page.waitForEvent('filechooser');await page.getByRole('button',{name:'Khôi phục từ file',exact:true}).click();await(await chooser).setFiles({name:'settings.json',mimeType:'application/json',buffer:Buffer.from(JSON.stringify(backup))});}
 await choose();await expect(page.getByRole('dialog',{name:'Khôi phục setting',exact:true})).toContainText('18001');await page.getByRole('button',{name:'Giữ setting hiện tại',exact:true}).click();
 await expect(page.locator('.check-settings')).not.toContainText('Restored portal');await choose();await page.getByRole('button',{name:'Xác nhận khôi phục',exact:true}).click();
 await expect(page.locator('.check-settings')).toContainText('Restored portal');await expect(page.getByRole('combobox',{name:'Đơn vị mặc định',exact:true})).toHaveText('MiB/s');
 await page.getByRole('button',{name:'Quy tắc',exact:true}).click();await expect(page.locator('.rule-row')).toHaveCount(1);await expect(page.locator('.rule-row')).toContainText('restored.example.test');
});
test('invalid file leaves configuration unchanged and active sessions disable restore',async({page})=>{
 await page.goto('/');await page.getByRole('button',{name:'Cài đặt',exact:true}).click();const chooser=page.waitForEvent('filechooser');await page.getByRole('button',{name:'Khôi phục từ file',exact:true}).click();await(await chooser).setFiles({name:'invalid.json',mimeType:'application/json',buffer:Buffer.from('{"version":99}')});
 await expect(page.getByRole('alert')).toContainText('File sao lưu không hợp lệ');await expect(page.getByRole('dialog')).toHaveCount(0);
 await page.getByRole('button',{name:'Tổng quan',exact:true}).click();await page.getByRole('button',{name:'Bật phân luồng',exact:true}).click();await page.getByRole('button',{name:'Cài đặt',exact:true}).click();await expect(page.getByRole('button',{name:'Khôi phục từ file',exact:true})).toBeDisabled();
});

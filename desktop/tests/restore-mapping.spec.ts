import {test,expect,Page} from '@playwright/test';
const backup={format:'netbridge-settings',version:1,config:{port:18001,lan_id:'old-lan',usb_id:'old-usb',adb_path:'C:\\Example\\adb.exe',rules:[{pattern:'portal.example.test',route:'lan'}],check_targets:[]},preferences:{unit:'MB/s'}};
async function importFile(page:Page){const chooser=page.waitForEvent('filechooser');await page.getByRole('button',{name:'Khôi phục từ file',exact:true}).click();await(await chooser).setFiles({name:'mapping.json',mimeType:'application/json',buffer:Buffer.from(JSON.stringify(backup))});}
test.beforeEach(async({page})=>{await page.goto('/');await page.getByRole('button',{name:'Cài đặt',exact:true}).click();});
test('restore maps foreign adapters and can switch to bundled ADB',async({page})=>{
 await importFile(page);const lan=page.getByRole('combobox',{name:'Adapter LAN sau khôi phục',exact:true});const usb=page.getByRole('combobox',{name:'Adapter USB sau khôi phục',exact:true});
 await expect(lan).toContainText('đã lưu');await expect(usb).toContainText('đã lưu');
 await lan.click();await page.getByRole('option',{name:'Ethernet · 192.0.2.10',exact:true}).click();await usb.click();await page.getByRole('option',{name:'Ethernet 3 · 198.51.100.10',exact:true}).click();
 await page.getByRole('checkbox',{name:'Dùng ADB tích hợp trên máy này',exact:true}).check();await page.getByRole('button',{name:'Xác nhận khôi phục',exact:true}).click();
 await page.locator('details.advanced summary').click();await expect(page.locator('#adb-path')).toHaveValue('auto');await page.getByRole('button',{name:'Kết nối',exact:true}).click();await expect(page.locator('#lan')).toHaveText('Ethernet · 192.0.2.10');await expect(page.locator('#usb')).toHaveText('Ethernet 3 · 198.51.100.10');
});
test('cancel discards mapping edits and duplicate roles warn',async({page})=>{
 await importFile(page);for(const label of ['Adapter LAN sau khôi phục','Adapter USB sau khôi phục']){await page.getByRole('combobox',{name:label,exact:true}).click();await page.getByRole('option',{name:'Ethernet · 192.0.2.10',exact:true}).click();}
 await expect(page.getByRole('dialog')).toContainText('cùng một adapter');await page.getByRole('button',{name:'Giữ setting hiện tại',exact:true}).click();await page.getByRole('button',{name:'Kết nối',exact:true}).click();await expect(page.locator('#usb')).toHaveText('Ethernet 3 · 198.51.100.10');
});
test('offline saved IDs and custom ADB are preserved without auto-mapping',async({page})=>{
 await importFile(page);await expect(page.getByRole('dialog')).toContainText('chưa hoạt động');await page.getByRole('button',{name:'Xác nhận khôi phục',exact:true}).click();
 const pending=page.waitForEvent('download');await page.getByRole('button',{name:'Sao lưu setting',exact:true}).click();const stream=await(await pending).createReadStream();const parts:Buffer[]=[];for await(const part of stream!)parts.push(part);const restored=JSON.parse(Buffer.concat(parts).toString('utf8'));expect(restored.config.lan_id).toBe('old-lan');expect(restored.config.usb_id).toBe('old-usb');expect(restored.config.adb_path).toBe(backup.config.adb_path);
});

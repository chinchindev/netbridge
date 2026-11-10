import {test,expect} from '@playwright/test';
test.beforeEach(async({page})=>{await page.goto('/');await expect(page.getByRole('heading',{name:'Tổng quan',exact:true})).toBeVisible();});
test('navigation does not commit a draft',async({page})=>{
 await page.getByRole('button',{name:'Kết nối',exact:true}).click();
 await page.locator('#lan').click();
 await page.getByRole('option',{name:'Ethernet 3 · 192.168.183.100',exact:true}).click();
 await expect(page.getByText('Có thay đổi chưa áp dụng')).toBeVisible();
 await expect(page.getByText(/LAN và USB đang trỏ cùng/)).toBeVisible();
 await page.getByRole('button',{name:'Đo mạng',exact:true}).click();
 await page.getByRole('button',{name:'Kết nối',exact:true}).click();
 await expect(page.locator('#lan')).toHaveText('Ethernet 3 · 192.168.183.100');
 await page.getByRole('button',{name:'Hủy thay đổi',exact:true}).click();
 await expect(page.locator('#lan')).toHaveText('Ethernet · 10.3.80.189');
});
test('external profile conflict is explicit and requires a reload',async({page})=>{
 await page.goto('/?scenario=external-config');
 await expect(page.getByText('File cấu hình đã thay đổi ngoài ứng dụng.')).toBeVisible();
 await page.getByRole('button',{name:'Tải cấu hình mới',exact:true}).click();
 await expect(page.getByText('File cấu hình đã thay đổi ngoài ứng dụng.')).toHaveCount(0);
});
test('rule editing validates, orders, previews and saves',async({page})=>{
 await page.getByRole('button',{name:'Quy tắc',exact:true}).click();
 await page.locator('#rule-pattern').fill('bad host');
 await page.getByRole('button',{name:'Thêm',exact:true}).click();
 await expect(page.getByText(/không có khoảng trắng/)).toBeVisible();
 await page.locator('#rule-pattern').fill('*.example.dev');
 await page.locator('#rule-route').click();
 await page.getByRole('option',{name:'Internet USB',exact:true}).click();
 await page.getByRole('button',{name:'Thêm',exact:true}).click();
 await page.getByRole('button',{name:'Đưa *.example.dev lên',exact:true}).click();
 await page.getByRole('textbox',{name:'Hostname cần thử'}).fill('api.example.dev');
 await page.getByRole('button',{name:'Kiểm tra quy tắc',exact:true}).click();
 await expect(page.locator('.probe-result')).toContainText('Internet USB');
 await expect(page.locator('.probe-result')).toContainText('Quy tắc #4');
 await page.getByRole('button',{name:'Lưu thay đổi',exact:true}).click();
 await expect(page.getByText('Có thay đổi chưa áp dụng')).toHaveCount(0);
 await expect(page.locator('.rule-row')).toHaveCount(5);
});
test('stop is explicit and cancellation keeps routing active',async({page})=>{
 await page.getByRole('button',{name:'Bật phân luồng',exact:true}).click();
 await expect(page.locator('.session-copy h2')).toHaveText('Đang phân luồng');
 await page.getByRole('button',{name:'Tắt phân luồng',exact:true}).click();
 await expect(page.getByRole('dialog',{name:'Tắt phân luồng',exact:true})).toBeVisible();
 await page.getByRole('button',{name:'Giữ phiên hiện tại'}).click();
 await expect(page.locator('.session-copy h2')).toHaveText('Đang phân luồng');
});
test('speed units transform the same result and explain assessment',async({page})=>{
 await page.getByRole('button',{name:'Đo mạng',exact:true}).click();
 await page.getByRole('button',{name:'Đo tốc độ',exact:true}).click();
 await expect(page.locator('.speed-card')).toHaveCount(2);
 await expect(page.locator('[data-testid=download-value]').first()).toContainText('154,80');
 await page.locator('#unit').click();
 await page.getByRole('option',{name:'MB/s',exact:true}).click();
 await expect(page.locator('[data-testid=download-value]').first()).toContainText('19,35');
 await expect(page.locator('.grade').first()).toHaveText('Rất tốt');
 await page.getByRole('button',{name:'Tiêu chí đánh giá mạng'}).click();
 await expect(page.getByRole('dialog')).toContainText('≥100 Mbps');
 await page.keyboard.press('Escape');
 await expect(page.getByRole('dialog')).toHaveCount(0);
});
test('all screens fit a compact desktop',async({page})=>{
 await page.setViewportSize({width:960,height:680});
 for(const name of ['Tổng quan','Kết nối','Quy tắc','Đo mạng','Cài đặt']){
  await page.getByRole('button',{name,exact:true}).click();
  expect(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBeTruthy();
 }
});
test('dialog receives keyboard focus and restores it when closed',async({page})=>{
 await page.getByRole('button',{name:'Đo mạng',exact:true}).click();
 const trigger=page.getByRole('button',{name:'Tiêu chí đánh giá mạng'});
 await trigger.click();
 await expect(page.getByRole('button',{name:'Đóng hộp thoại'})).toBeFocused();
 await page.keyboard.press('Shift+Tab');
 await expect(page.getByRole('button',{name:'Đã hiểu'})).toBeFocused();
 await page.keyboard.press('Escape');await expect(trigger).toBeFocused();
});
test('capture design states',async({page})=>{
 for(const [name,file] of [['Tổng quan','overview'],['Kết nối','connections'],['Quy tắc','rules'],['Đo mạng','speed'],['Cài đặt','settings']]){
  await page.getByRole('button',{name,exact:true}).click();
  if(file==='speed'){await page.getByRole('button',{name:'Đo tốc độ',exact:true}).click();await expect(page.locator('.speed-card')).toHaveCount(2);}
  await page.screenshot({path:'../.ui-snapshots/2026-11-09-desktop-v2/'+file+'.png'});
 }
});

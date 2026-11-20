import {expect,test} from '@playwright/test';

test('monitor samples each adapter and filters connections and listeners separately',async({page},testInfo)=>{
 await page.goto('/');
 await page.getByRole('button',{name:'Giám sát mạng',exact:true}).click();
 const picker=page.getByRole('combobox',{name:'Card đang theo dõi'});
 await expect(picker).toHaveText('Ethernet');
 await picker.click();
 await page.getByRole('option',{name:'Ethernet 3',exact:true}).click();
 await expect(page.getByRole('img',{name:'Biểu đồ lưu lượng nhận và gửi theo thời gian'})).toBeVisible({timeout:9000});
 await expect(page.locator('tbody')).toContainText('example.exe');
 await page.getByRole('button',{name:'Cổng lắng nghe',exact:true}).click();
 await expect(page.locator('tbody')).toContainText('127.0.0.1:17890');
 const filter=page.getByRole('textbox',{name:'Lọc kết nối TCP'});
 await filter.fill('absent');
 await expect(page.locator('tbody tr')).toHaveCount(0);
 await filter.fill('420');
 await expect(page.locator('tbody tr')).toHaveCount(1);
 await page.getByRole('button',{name:'Tạm dừng',exact:true}).click();
 await expect(page.getByRole('button',{name:'Tiếp tục',exact:true})).toBeVisible();
 await page.screenshot({path:testInfo.outputPath('monitor.png')});
 await page.getByRole('button',{name:'Tiếp tục',exact:true}).click();
 await expect(page.getByRole('button',{name:'Tạm dừng',exact:true})).toBeVisible();
});

test('Teams includes WebView2 tunnel evidence, USB checks and the incident timeline',async({page},testInfo)=>{
 await page.goto('/');
 await page.getByRole('button',{name:'Giám sát mạng',exact:true}).click();
 await page.getByRole('combobox',{name:'Ứng dụng',exact:true}).click();
 await page.getByRole('option',{name:'ms-teams.exe',exact:true}).click();
 await expect(page.locator('tbody')).toContainText('msedgewebview2.exe');
 await expect(page.locator('tbody tr')).toHaveCount(1);
 await page.getByRole('button',{name:/NetBridge → USB/}).click();
 await expect(page.locator('tbody')).toContainText('example.test:443');
 await page.getByRole('button',{name:'Kiểm tra đường USB'}).click();
 await expect(page.locator('.usb-health-list li')).toHaveCount(4);
 await expect(page.locator('.monitor-events')).toContainText('Ethernet 3');
 await page.locator('.usb-health-list').scrollIntoViewIfNeeded();
 await page.screenshot({path:testInfo.outputPath('evidence.png')});
});

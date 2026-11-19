import {test,expect} from '@playwright/test';
test('combined USB action reaches an active routing session',async({page})=>{
 await page.goto('/');await page.getByRole('button',{name:'Kết nối',exact:true}).click();
 await page.getByRole('button',{name:'Kết nối USB & phân luồng',exact:true}).click();
 await page.getByRole('button',{name:'Xác nhận kết nối',exact:true}).click();
 await page.getByRole('button',{name:'Tổng quan',exact:true}).click();await expect(page.locator('.session-copy h2')).toHaveText('Đang phân luồng');
});
test('combined action requires a saved draft and reports failed routing',async({page})=>{
 await page.goto('/?scenario=usb-connect-failure');await page.getByRole('button',{name:'Kết nối',exact:true}).click();
 const combined=page.getByRole('button',{name:'Kết nối USB & phân luồng',exact:true});await page.locator('#lan').click();await page.getByRole('option',{name:'Ethernet 3 · 198.51.100.10',exact:true}).click();await expect(combined).toBeDisabled();
 await page.getByRole('button',{name:'Hủy thay đổi',exact:true}).click();await combined.click();await page.getByRole('button',{name:'Xác nhận kết nối',exact:true}).click();
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

test('multiple USB cards require an explicit choice and allow cancellation',async({page})=>{
 await page.goto('/?scenario=multiple-usb');
 await page.getByRole('button',{name:'Kết nối',exact:true}).click();
 const connect=page.getByRole('button',{name:'Kết nối USB & phân luồng',exact:true});
 await connect.click();
 const dialog=page.getByRole('dialog',{name:'Xác nhận đường USB'});
 const confirm=dialog.getByRole('button',{name:'Xác nhận kết nối'});
 await expect(confirm).toBeDisabled();
 const picker=dialog.getByRole('combobox',{name:'Card Windows và IP nguồn của điện thoại'});
 await picker.click();
 await expect(dialog.getByRole('option')).toHaveCount(2);
 await dialog.getByRole('option',{name:'Ethernet 4 · 203.0.113.10'}).click();
 await expect(confirm).toBeEnabled();
 await dialog.getByRole('button',{name:'Hủy',exact:true}).click();
 await connect.click();
 await expect(confirm).toBeDisabled();
 await picker.focus();
 await picker.press('ArrowDown');
 await picker.press('Escape');
 await expect(dialog).toBeVisible();
 await expect(confirm).toBeDisabled();
 await picker.press('Enter');
 await picker.press('End');
 await picker.press('Enter');
 await expect(picker).toHaveText('Ethernet 4 · 203.0.113.10');
 await confirm.click();
 await expect(page.locator('#usb')).toContainText('Ethernet 4');
});

test('diagnostics shows backup status and supports filtering connections',async({page})=>{
 await page.goto('/');
 await page.getByRole('button',{name:'Chẩn đoán',exact:true}).click();
 const drawer=page.getByRole('dialog',{name:'Nhật ký và chẩn đoán'});
 await expect(drawer).toContainText('Bản sao proxy: 0');
 await drawer.getByRole('button',{name:'Kết nối',exact:true}).click();
 await drawer.getByLabel('Lọc hostname').fill('example.test');
 await expect(drawer).toContainText('Chưa có kết nối phù hợp.');
 await expect(drawer.getByRole('button',{name:'Sao chép báo cáo ẩn danh'})).toBeEnabled();
});

test('connection export omits private fields even for a failed tunnel',async({page,context},testInfo)=>{
 await context.grantPermissions(['clipboard-read','clipboard-write']);
 await page.addInitScript(()=>{
  const snapshot={mode:'preview',version:'0.7.0',configRevision:'sample',configChanged:false,
   config:{port:17890,lan_id:'',usb_id:'',adb_path:'auto',rules:[],check_targets:[]},adapters:[],
   session:{kind:'inactive',label:'Phiên kiểm thử',detail:'',owned:false,endpoint:'127.0.0.1:17890'},
   job:{id:0,kind:'',phase:'',active:false,cancelling:false,error:null},targets:[],speeds:[],logs:[],unit:'Mbps'};
  const row={id:1,started:1,port:443,host:'private.example.test',adapter:'private-adapter',source:'192.0.2.77',
   stage:'TCP',result:'private-error',route:'usb',rule:2,elapsedMs:83,sent:13,received:8};
  const replies:Record<string,unknown>={get_snapshot:snapshot,get_app_update:{status:''},
   diagnostic_details:{traffic:{active:0,rejected:2,rows:[row]},watchdog:null,backups:{count:0,latest:null,bytes:0}}};
  Object.assign(window,{__TAURI_INTERNALS__:{transformCallback:()=>1,invoke:async(cmd:string)=>replies[cmd]??1}});
 });
 await page.goto('/');
 await page.getByRole('button',{name:'Chẩn đoán',exact:true}).click();
 const drawer=page.getByRole('dialog',{name:'Nhật ký và chẩn đoán'});
 await drawer.getByRole('button',{name:'Kết nối',exact:true}).click();
 const rows=drawer.locator('.log-connection');
 await expect(rows).toContainText('private.example.test');
 await expect(rows).toContainText('Quy tắc #2');
 await drawer.getByLabel('Lọc hostname').fill('other.example');
 await expect(rows).toHaveCount(0);
 await drawer.getByLabel('Lọc hostname').fill('PRIVATE');
 await expect(rows).toHaveCount(1);
 await page.screenshot({path:testInfo.outputPath('connections.png')});
 await drawer.getByRole('button',{name:'Sao chép báo cáo ẩn danh'}).click();
 await expect.poll(()=>page.evaluate(()=>navigator.clipboard.readText())).toContain('interrupted');
 const report=JSON.parse(await page.evaluate(()=>navigator.clipboard.readText()));
 expect(report.connections).toEqual([{stage:'TCP',route:'usb',rule:2,durationMs:83,sent:13,received:8,outcome:'interrupted'}]);
 expect(JSON.stringify(report)).not.toMatch(/private|192\.0\.2\.77/i);
});

test('paired phones select their own Windows card despite a different saved adapter',async({page})=>{
 await page.goto('/?scenario=paired-usb');
 await page.getByRole('button',{name:'Kết nối',exact:true}).click();
 const connect='Kết nối USB & phân luồng';
 const dialog=page.getByRole('dialog',{name:'Xác nhận đường USB'});
 await page.locator('.device-row').filter({hasText:'Android_USB'}).getByRole('button',{name:connect}).click();
 await expect(dialog).toContainText('Ethernet 3');
 await dialog.getByRole('button',{name:'Hủy',exact:true}).click();
 await page.locator('.device-row').filter({hasText:'Android_B'}).getByRole('button',{name:connect}).click();
 await expect(dialog).toContainText('Ethernet 4');
 await expect(dialog).toContainText('203.0.113.10');
 await expect(dialog.getByRole('combobox')).toHaveCount(0);
 await dialog.getByRole('button',{name:'Xác nhận kết nối'}).click();
 await expect(page.locator('#usb')).toContainText('Ethernet 4');
});

import {test,expect} from '@playwright/test';

test.beforeEach(async({page})=>{
 await page.goto('/');
 await page.getByRole('button',{name:'Quy tắc',exact:true}).click();
});

test('cancel editing clears the editor and adds the next rule without replacing the original',async({page})=>{
 await page.getByRole('button',{name:'*.example.org',exact:true}).click();
 await page.locator('#rule-pattern').fill('bad host');
 await page.getByRole('button',{name:'Cập nhật',exact:true}).click();
 await expect(page.locator('.field-error')).toContainText('không có khoảng trắng');
 await page.getByRole('button',{name:'Hủy sửa',exact:true}).click();
 await expect(page.locator('#rule-pattern')).toHaveValue('');
 await expect(page.locator('#rule-pattern')).toBeFocused();
 await expect(page.getByLabel('Thêm quy tắc',{exact:true})).toBeVisible();
 await expect(page.getByRole('button',{name:'Hủy sửa',exact:true})).toHaveCount(0);
 await expect(page.getByRole('button',{name:'Cập nhật',exact:true})).toHaveCount(0);
 await expect(page.locator('.field-error')).toHaveCount(0);
 await expect(page.locator('.rule-row.editing')).toHaveCount(0);
 await expect(page.getByText('Có thay đổi chưa áp dụng')).toHaveCount(0);
 await page.locator('#rule-pattern').fill('new.example.test');
 await page.getByRole('button',{name:'Thêm',exact:true}).click();
 await expect(page.locator('.rule-row')).toHaveCount(5);
 await expect(page.getByRole('button',{name:'*.example.org',exact:true})).toBeVisible();
 await expect(page.getByRole('button',{name:'new.example.test',exact:true})).toBeVisible();
 await expect(page.locator('.rule-row').filter({has:page.getByRole('button',{name:'new.example.test',exact:true})})).toContainText('USB');
});

test('cancel editing preserves other unsaved rules and fits a compact window',async({page})=>{
 await page.setViewportSize({width:960,height:680});
 await page.locator('#rule-pattern').fill('pending.example.test');
 await page.getByRole('button',{name:'Thêm',exact:true}).click();
 await page.getByRole('button',{name:'portal.example.test',exact:true}).click();
 await page.locator('#rule-pattern').fill('discarded.example.test');
 const cancel=page.getByRole('button',{name:'Hủy sửa',exact:true});
 await cancel.scrollIntoViewIfNeeded();
 await expect(cancel).toBeInViewport();
 await expect(page.getByRole('button',{name:'Cập nhật',exact:true})).toBeInViewport();
 expect(await page.locator('.rule-editor').evaluate(el=>el.scrollWidth<=el.clientWidth)).toBeTruthy();
 await cancel.click();
 await expect(page.getByText('Có thay đổi chưa áp dụng')).toBeVisible();
 await expect(page.getByRole('button',{name:'pending.example.test',exact:true})).toBeVisible();
 await expect(page.getByRole('button',{name:'portal.example.test',exact:true})).toBeVisible();
 await expect(page.getByRole('button',{name:'discarded.example.test',exact:true})).toHaveCount(0);
 await page.getByRole('button',{name:'Lưu thay đổi',exact:true}).click();
 await expect(page.getByText('Có thay đổi chưa áp dụng')).toHaveCount(0);
 await expect(page.locator('.rule-row')).toHaveCount(5);
});

test('invalid URL suggests its hostname and leaves the saved list intact',async({page})=>{
 const field=page.locator('#rule-pattern');
 await field.fill('https://example.test/path?source=demo');
 await page.getByRole('button',{name:'Thêm',exact:true}).click();
 await expect(page.locator('.field-error')).toContainText('Có thể nhập example.test.');
 await expect(page.locator('.rule-row')).toHaveCount(4);
 await field.fill('203.0.113.0/24');
 await page.getByRole('button',{name:'Thêm',exact:true}).click();
 await expect(page.getByRole('button',{name:'203.0.113.0/24',exact:true})).toBeVisible();
});

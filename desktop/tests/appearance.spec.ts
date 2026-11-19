import {expect,test} from '@playwright/test';

test('appearance follows Windows or the saved user choice across restarts',async({page})=>{
 await page.emulateMedia({colorScheme:'dark'});
 await page.goto('/');
 const root=page.locator('html');
 await expect(root).toHaveAttribute('data-theme','dark');
 await page.getByRole('button',{name:'Cài đặt',exact:true}).click();
 const appearance=page.getByRole('combobox',{name:'Giao diện'});
 await appearance.click();
 await page.getByRole('option',{name:'Sáng',exact:true}).click();
 await expect(root).toHaveAttribute('data-theme','light');
 await page.reload();
 await expect(root).toHaveAttribute('data-theme','light');
 await page.getByRole('button',{name:'Cài đặt',exact:true}).click();
 await appearance.click();
 await page.getByRole('option',{name:'Theo Windows'}).click();
 await expect(root).toHaveAttribute('data-theme','dark');
 await page.emulateMedia({colorScheme:'light'});
 await expect(root).toHaveAttribute('data-theme','light');
 await appearance.click();
 await page.getByRole('option',{name:'Tối',exact:true}).click();
 await page.reload();
 await expect(root).toHaveAttribute('data-theme','dark');
 await expect(page.locator('.sidebar')).toHaveCSS('background-color','rgb(25, 35, 49)');
});

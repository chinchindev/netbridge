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

test('a botanical palette keeps its mode when Windows changes and clears on return to system',async({page})=>{
 await page.emulateMedia({colorScheme:'dark'});
 await page.goto('/');
 await page.getByRole('button',{name:'Cài đặt',exact:true}).click();
 const picker=page.getByRole('combobox',{name:'Giao diện'});
 const root=page.locator('html');
 for(const [label,id,mode] of [
  ['Anh đào · Sáng','cherry','light'],['Cúc họa mi · Sáng','meadow','light'],
  ['Dương xỉ · Tối','fern','dark'],['Phong lan · Tối','orchid','dark']
 ]){
  await picker.click();
  await page.getByRole('option',{name:label,exact:true}).click();
  await expect(root).toHaveAttribute('data-palette',id);
  await expect(root).toHaveAttribute('data-theme',mode);
  await expect(root).toHaveCSS('color-scheme',mode);
 }
 await page.emulateMedia({colorScheme:'light'});
 await expect(root).toHaveAttribute('data-theme','dark');
 await picker.click();
 await page.getByRole('option',{name:'Theo Windows',exact:true}).click();
 await expect(root).toHaveAttribute('data-theme','light');
 await expect(root).not.toHaveAttribute('data-palette',/.+/);
});

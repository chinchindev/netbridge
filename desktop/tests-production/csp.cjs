const {chromium,expect}=require('@playwright/test');
const fs=require('node:fs');
const path=require('node:path');
const http=require('node:http');
const crypto=require('node:crypto');
const root=path.resolve(__dirname,'../dist/browser');
const output=path.resolve(process.env.NETBRIDGE_QA_OUTPUT||'../.qa/desktop-v2-2026-11-10/production');
fs.mkdirSync(output,{recursive:true});
const nonce=crypto.randomBytes(24).toString('base64');
const config=JSON.parse(fs.readFileSync(path.resolve(__dirname,'../src-tauri/tauri.conf.json'),'utf8'));
// Tauri adds nonces to static style/script tags. Angular runtime styles must also be allowed.
const csp=config.app.security.csp.replace("script-src 'self'",`script-src 'self' 'nonce-${nonce}'`).replace("style-src 'self'",`style-src 'self' 'nonce-${nonce}'`);
const server=http.createServer((req,res)=>{
 const name=new URL(req.url,'http://localhost').pathname;
 if(name!=='/'&&!/^\/(?:media\/)?[\w.-]+$/.test(name)){res.writeHead(404).end();return;}
 const file=path.join(root,name==='/'?'index.html':name.slice(1));
 if(!fs.existsSync(file)){res.writeHead(404).end();return;}
 const ext=path.extname(file);
 res.setHeader('Content-Type',({'.html':'text/html; charset=utf-8','.js':'text/javascript','.css':'text/css','.png':'image/png'})[ext]||'application/octet-stream');
 res.setHeader('Content-Security-Policy',csp);
 let data=fs.readFileSync(file);
 if(ext==='.html')data=data.toString('utf8').replace(/<(style|script)(\s|>)/g,`<$1 nonce="${nonce}"$2`);
 res.end(data);
});
(async()=>{
 await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
 const browser=await chromium.launch({channel:process.env.CI?undefined:'msedge',headless:true});
 const page=await browser.newPage({viewport:{width:1240,height:860}});
 const report={policy:csp,checks:[],errors:[],violations:[]};
 page.on('pageerror',error=>report.errors.push(error.message));
 await page.addInitScript(()=>{window.qaViolations=[];document.addEventListener('securitypolicyviolation',e=>window.qaViolations.push({directive:e.effectiveDirective,blocked:e.blockedURI}));});
 try{
  await page.goto(`http://127.0.0.1:${server.address().port}`);
  await expect(page.getByRole('heading',{name:'Tổng quan',exact:true})).toBeVisible();
  await page.screenshot({path:path.join(output,'overview.png')});
  const size=await page.locator('.small-symbol nb-icon').boundingBox();
  report.icon=size;
  expect(size.width).toBe(20);expect(size.height).toBe(20);
  report.checks.push('icon size with production CSP');
  await page.getByRole('button',{name:'Đo mạng',exact:true}).click();
  await page.getByRole('button',{name:'Đo tốc độ',exact:true}).click();
  await expect(page.locator('.speed-card')).toHaveCount(2);
  const widths=await page.locator('.bar i').evaluateAll(items=>items.map(el=>el.getBoundingClientRect().width));
  expect(widths.every(w=>w>0)).toBeTruthy();
  report.checks.push('dynamic measurement bars');
  await page.getByRole('combobox',{name:'Hiển thị',exact:true}).click();
  await expect(page.getByRole('listbox')).toBeVisible();
  await page.getByRole('option',{name:'MB/s',exact:true}).click();
  await expect(page.locator('[data-testid=download-value]').first()).toContainText('19,35');
  report.checks.push('styled dropdown selection with production CSP');
  await page.screenshot({path:path.join(output,'speed.png')});
  await page.getByRole('button',{name:'Cài đặt',exact:true}).click();
  const pendingDownload=page.waitForEvent('download');
  await page.getByRole('button',{name:'Sao lưu setting',exact:true}).click();
  const exported=await pendingDownload;
  const stream=await exported.createReadStream();const chunks=[];for await(const chunk of stream)chunks.push(chunk);
  expect(JSON.parse(Buffer.concat(chunks).toString('utf8')).format).toBe('netbridge-settings');
  report.checks.push('settings export with production CSP');
  const presets=[['cherry','Anh đào · Sáng'],['meadow','Cúc họa mi · Sáng'],['fern','Dương xỉ · Tối'],['orchid','Phong lan · Tối']];
  for(const [id,label] of presets){
   await page.getByRole('button',{name:'Cài đặt',exact:true}).click();
   await page.getByRole('combobox',{name:'Giao diện'}).click();
   await page.getByRole('option',{name:label,exact:true}).click();
   await page.reload();
   await expect(page.locator('html')).toHaveAttribute('data-palette',id);
   const loaded=await page.locator('.page-heading').evaluate(async heading=>{
    const url=getComputedStyle(heading).backgroundImage.match(/url\("([^"]+)"\)/)?.[1];
    if(!url)return false;
    const image=new Image();image.src=url;await image.decode();return image.naturalWidth>0;
   });
   expect(loaded).toBeTruthy();
   await page.screenshot({path:path.join(output,id+'.png')});
  }
  report.checks.push('four botanical palettes persist and load their bundled artwork with production CSP');
  report.violations=await page.evaluate(()=>window.qaViolations);
  expect(report.violations).toHaveLength(0);expect(report.errors).toHaveLength(0);
  report.passed=true;
 }catch(error){report.passed=false;report.failure=String(error);report.violations=await page.evaluate(()=>window.qaViolations);process.exitCode=1;}
 finally{fs.writeFileSync(path.join(output,'result.json'),JSON.stringify(report,null,2),'utf8');console.log(JSON.stringify(report));await browser.close();server.close();}
})();

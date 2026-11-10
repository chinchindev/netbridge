import {defineConfig} from '@playwright/test';
export default defineConfig({
 testDir:'./tests',fullyParallel:false,workers:1,timeout:30000,
 use:{baseURL:'http://127.0.0.1:1420',channel:process.env['CI']?undefined:'msedge',headless:true,viewport:{width:1240,height:900},trace:'retain-on-failure'},
 webServer:{command:'npm run start',url:'http://127.0.0.1:1420',reuseExistingServer:!process.env['CI'],timeout:120000},
 reporter:[['list'],['json',{outputFile:'test-results/results.json'}]]
});

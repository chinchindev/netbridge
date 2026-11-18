import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import {versionInfo} from './version.mjs';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
const input={eventName:'workflow_dispatch',ref:'refs/heads/feature/installer',version:'0.6.0',baselineVersion:'0.5.0'};
async function context(){return (await import('./release-context.mjs')).releaseContext;}
test('manual branch builds produce candidate metadata without permission to publish',async()=>{
 assert.deepEqual((await context())(input),{tag:'v0.6.0',baselineVersion:'0.5.0',publish:false});
});
test('only a matching tag push may publish',async()=>{
 const get=await context();
 assert.deepEqual(get({...input,eventName:'push',ref:'refs/tags/v0.6.0'}),{tag:'v0.6.0',baselineVersion:'0.5.0',publish:true});
 for(const value of [{eventName:'push',ref:'refs/heads/main'},{eventName:'push',ref:'refs/tags/v0.5.0'},{eventName:'pull_request'},{eventName:'workflow_dispatch',ref:'refs/tags/v0.6.0'}])assert.throws(()=>get({...input,...value}));
});
test('baseline must be a canonical older stable version',async()=>{
 const get=await context();
 for(const baselineVersion of ['','0.6.0','0.7.0','0.5.0-rc.1','00.5.0','../0.5.0','0.5.0\nPUBLISH=true'])assert.throws(()=>get({...input,baselineVersion}));
 assert.equal(get({...input,version:'0.10.0',baselineVersion:'0.9.0'}).baselineVersion,'0.9.0');
 assert.equal(get({...input,version:'0.6.0-rc.1'}).publish,false);
});
test('CLI rejects a mismatched tag before writing GitHub environment',()=>{
 const directory=path.join(root,'.qa','release-context-tests');fs.mkdirSync(directory,{recursive:true});
 const output=path.join(directory,'environment-'+Date.now()+'.txt');fs.writeFileSync(output,'SENTINEL=kept\n');
 const result=spawnSync(process.execPath,[path.join(root,'scripts/release-context.mjs')],{encoding:'utf8',env:{...process.env,GITHUB_EVENT_NAME:'push',GITHUB_REF:'refs/tags/v999.0.0',BASELINE_VERSION:'0.5.0',GITHUB_ENV:output}});
 assert.notEqual(result.status,0);assert.match(result.stderr,/does not match/);assert.equal(fs.readFileSync(output,'utf8'),'SENTINEL=kept\n');
});
test('manual CLI exports validated candidate and baseline for subsequent build steps',()=>{
 const directory=path.join(root,'.qa','release-context-tests');fs.mkdirSync(directory,{recursive:true});
 const output=path.join(directory,'manual-environment-'+Date.now()+'.txt');fs.writeFileSync(output,'SENTINEL=kept\n');
 const result=spawnSync(process.execPath,[path.join(root,'scripts/release-context.mjs')],{encoding:'utf8',env:{...process.env,GITHUB_EVENT_NAME:'workflow_dispatch',GITHUB_REF:'refs/heads/main',BASELINE_VERSION:'0.5.0',GITHUB_ENV:output}});
 assert.equal(result.status,0,result.stderr);
 assert.equal(JSON.parse(result.stdout).publish,false);
 assert.equal(fs.readFileSync(output,'utf8'),`SENTINEL=kept\nNETBRIDGE_RELEASE_TAG=v${versionInfo()}\nBASELINE_VERSION=0.5.0\n`);
});
test('installer smoke refuses local execution before touching an installer',{skip:process.platform!=='win32'},()=>{
 const result=spawnSync('powershell.exe',['-NoProfile','-ExecutionPolicy','Bypass','-File',path.join(root,'scripts/windows-install-smoke.ps1'),'-Installer','does-not-exist.exe','-Version','0.6.0'],{encoding:'utf8',env:{...process.env,GITHUB_ACTIONS:'false',RUNNER_OS:'Windows'}});
 assert.notEqual(result.status,0);assert.match(result.stderr,/only run on an isolated GitHub Windows runner/);
});

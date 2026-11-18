import test from 'node:test';
import assert from 'node:assert/strict';
import {manifest} from './release-manifest.mjs';
import {versionInfo,assertTag} from './version.mjs';
const input={repository:'example/netbridge',tag:'v0.3.0',version:'0.3.0',file:'NetBridge_0.3.0_x64-setup.exe',signature:'signed bytes'};
test('release manifest uses HTTPS and the signed installer for its exact tag',()=>{
 const result=manifest(input);assert.equal(result.platforms['windows-x86_64'].url,'https://github.com/example/netbridge/releases/download/v0.3.0/NetBridge_0.3.0_x64-setup.exe');assert.equal(result.platforms['windows-x86_64'].signature,'signed bytes');
});
test('tag mismatch and missing signatures cannot be published',()=>{
 assert.throws(()=>manifest({...input,tag:'v0.2.0'}));assert.throws(()=>manifest({...input,signature:' '}));
});
test('repository/path injection is rejected',()=>{
 assert.throws(()=>manifest({...input,repository:'example/repo/other'}));assert.throws(()=>manifest({...input,file:'../other-setup.exe'}));
});
test('all application version files agree',()=>{const version=versionInfo();assertTag(version,'v'+version);});

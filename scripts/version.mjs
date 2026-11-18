import fs from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
const valid=/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?$/;
export function versionInfo(directory=root){
 const read=name=>fs.readFileSync(path.join(directory,name),'utf8').replace(/^\uFEFF/,'');
 const pkg=JSON.parse(read('desktop/package.json')),lock=JSON.parse(read('desktop/package-lock.json'));
 const config=JSON.parse(read('desktop/src-tauri/tauri.conf.json'));
 const cargo=read('desktop/src-tauri/Cargo.toml').match(/\[package\][\s\S]*?\nversion\s*=\s*"([^"]+)"/)[1];
 const cargoLock=read('desktop/src-tauri/Cargo.lock').match(/name = "netbridge-desktop"\r?\nversion = "([^"]+)"/)[1];
 const versions=[pkg.version,lock.version,lock.packages[''].version,config.version,cargo,cargoLock];
 if(!versions.every(version=>version===pkg.version)||!valid.test(pkg.version))throw new Error('App versions do not match: '+versions.join(', '));
 return pkg.version;
}
export function assertTag(version,tag){if(tag!=='v'+version)throw new Error(`Tag ${tag} does not match v${version}`);}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url)){
 const next=process.argv[2];
 if(next&&next!=='--check'){
  if(!valid.test(next))throw new Error('Use a SemVer such as 0.3.0 or 0.3.0-rc.1');
  for(const name of ['desktop/package.json','desktop/package-lock.json','desktop/src-tauri/tauri.conf.json']){
   const file=path.join(root,name),value=JSON.parse(fs.readFileSync(file,'utf8'));
   value.version=next;if(value.packages?.[''])value.packages[''].version=next;
   fs.writeFileSync(file,JSON.stringify(value,null,2)+'\n','utf8');
  }
  const cargo=path.join(root,'desktop/src-tauri/Cargo.toml');
  fs.writeFileSync(cargo,fs.readFileSync(cargo,'utf8').replace(/(\[package\][\s\S]*?\nversion\s*=\s*")[^"]+"/,'$1'+next+'"'),'utf8');
  const lock=path.join(root,'desktop/src-tauri/Cargo.lock');
  fs.writeFileSync(lock,fs.readFileSync(lock,'utf8').replace(/(name = "netbridge-desktop"\r?\nversion = ")[^"]+"/,'$1'+next+'"'),'utf8');
 }
 const version=versionInfo();if(process.env.NETBRIDGE_RELEASE_TAG)assertTag(version,process.env.NETBRIDGE_RELEASE_TAG);
 console.log(version);
}

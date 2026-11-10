import fs from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {createHash} from 'node:crypto';
import {versionInfo,assertTag} from './version.mjs';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
export function manifest({repository,tag,version,file,signature,date=new Date().toISOString()}){
 if(!/^[\w.-]+\/[\w.-]+$/.test(repository))throw new Error('Invalid owner/repository');
 assertTag(version,tag);
 if(!file.endsWith('-setup.exe')||path.basename(file)!==file||file.includes('\\'))throw new Error('Expected an NSIS installer filename');
 if(!signature.trim())throw new Error('Missing updater signature');
 return {version,notes:`NetBridge ${version}`,pub_date:date,platforms:{'windows-x86_64':{signature:signature.trim(),url:`https://github.com/${repository}/releases/download/${encodeURIComponent(tag)}/${encodeURIComponent(file)}`}}};
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url)){
 const version=versionInfo(),repository=process.env.GITHUB_REPOSITORY,tag=process.env.NETBRIDGE_RELEASE_TAG;
 const bundle=path.join(root,'desktop/src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis');
 const file=`NetBridge_${version}_x64-setup.exe`,installer=path.join(bundle,file);
 const signature=fs.readFileSync(installer+'.sig','utf8');
 const data=manifest({repository,tag,version,file,signature});
 const output=path.join(root,'dist/github-release');fs.mkdirSync(output,{recursive:true});
 for(const name of [file,file+'.sig'])fs.copyFileSync(path.join(bundle,name),path.join(output,name));
 fs.copyFileSync(path.join(root,'desktop/src-tauri/target/x86_64-pc-windows-msvc/release/netbridge-desktop.exe'),path.join(output,'NetBridge.exe'));
 fs.copyFileSync(path.join(root,'desktop/dist/3rdpartylicenses.txt'),path.join(output,'FRONTEND-LICENSES.txt'));
 fs.copyFileSync(path.join(root,'assets/adb/NOTICE.txt'),path.join(output,'ADB-NOTICE.txt'));
 fs.copyFileSync(path.join(root,'THIRD_PARTY_NOTICES.md'),path.join(output,'THIRD_PARTY_NOTICES.md'));
 fs.writeFileSync(path.join(output,'latest.json'),JSON.stringify(data,null,2)+'\n');
 const sums=fs.readdirSync(output).filter(name=>name!=='SHA256SUMS.txt').sort().map(name=>createHash('sha256').update(fs.readFileSync(path.join(output,name))).digest('hex')+'  '+name).join('\n')+'\n';
 fs.writeFileSync(path.join(output,'SHA256SUMS.txt'),sums);
 console.log('Release assets: '+output);
}

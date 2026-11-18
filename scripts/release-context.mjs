import fs from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {versionInfo,assertTag} from './version.mjs';

export function releaseContext({eventName,ref,version,baselineVersion}){
 if(!/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(baselineVersion))throw new Error('Baseline must be a stable SemVer');
 const baseline=baselineVersion.split('.').map(BigInt),candidate=version.split('-')[0].split('.').map(BigInt);
 const difference=baseline.findIndex((value,index)=>value!==candidate[index]);
 if(difference<0||baseline[difference]>=candidate[difference])throw new Error('Baseline must be older than the candidate');
 const tag='v'+version;
 if(eventName==='push'&&ref?.startsWith('refs/tags/')){
  assertTag(version,ref.slice('refs/tags/'.length));
  return {tag,baselineVersion,publish:true};
 }
 if(eventName==='workflow_dispatch'&&ref?.startsWith('refs/heads/'))return {tag,baselineVersion,publish:false};
 throw new Error('Only tag pushes or manual branch verification are supported');
}

if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url)){
 const result=releaseContext({eventName:process.env.GITHUB_EVENT_NAME,ref:process.env.GITHUB_REF,version:versionInfo(),baselineVersion:process.env.BASELINE_VERSION});
 if(!process.env.GITHUB_ENV)throw new Error('GITHUB_ENV is required');
 fs.appendFileSync(process.env.GITHUB_ENV,`NETBRIDGE_RELEASE_TAG=${result.tag}\nBASELINE_VERSION=${result.baselineVersion}\n`,'utf8');
 console.log(JSON.stringify(result));
}

import {Injectable} from '@angular/core';
import {AppUpdate,Snapshot,Config,Unit,SettingsBackup,ImportPreview,demoTargets} from './model';
import packageInfo from '../../package.json';
const clone=<T>(value:T):T=>structuredClone(value);
@Injectable({providedIn:'root'})
export class Gateway {
 readonly native='__TAURI_INTERNALS__' in window;
 private demo:Snapshot={
  mode:'demo',version:packageInfo.version,configRevision:'0',configChanged:false,
  config:{port:17890,lan_id:'demo-lan',usb_id:'demo-usb',adb_path:'auto',rules:[
   {pattern:'portal.example.test',route:'lan'},{pattern:'192.0.2.20',route:'lan'},
   {pattern:'*.example.org',route:'usb'},{pattern:'198.51.100.20',route:'usb'}],check_targets:demoTargets.map(({name,host,port})=>({name,host,port}))},
  adapters:[
   {id:'demo-lan',name:'Ethernet',description:'LAN nội bộ',ip:'192.0.2.10',dns:['192.0.2.53'],index:1,if_type:6},
   {id:'demo-usb',name:'Ethernet 3',description:'Remote NDIS · Android_USB',ip:'198.51.100.10',dns:['198.51.100.53'],index:2,if_type:6}],
  session:{kind:'inactive',owned:false,label:'Sẵn sàng kết nối',detail:'Bật proxy để áp dụng quy tắc cho các ứng dụng dùng proxy Windows.',endpoint:'127.0.0.1:17890'},
  job:{id:0,kind:'',phase:'',active:false,cancelling:false,error:null},
  targets:[],speeds:[],logs:['NetBridge sẵn sàng. Dữ liệu trong trình duyệt là mô phỏng.'],unit:'Mbps',adbLatest:null
 };
 private timer:ReturnType<typeof setTimeout>|undefined;
 private appUpdate:AppUpdate={status:'',version:null,notes:null,downloaded:0,total:null,error:null};
 constructor(){if(!this.native){const scenario=new URLSearchParams(location.search).get('scenario');if(scenario==='multiple-usb'||scenario==='paired-usb')this.demo.adapters.push({...this.demo.adapters[1],id:'demo-second',name:'Ethernet 4',ip:'203.0.113.10'});if(scenario==='external-config'||scenario==='config-error-external-fix')this.demo.configChanged=true;if(scenario==='empty-settings'){this.demo.config.rules=[];this.demo.config.check_targets=[];}if(scenario?.startsWith('config-error')){this.demo.configError='Cấu hình JSON không hợp lệ.';this.demo.recoveryAvailable=scenario==='config-error';this.demo.configRevision='broken';}}}
 async invoke<T>(command:string,args:Record<string,unknown>={}):Promise<T>{
  if(this.native){const {invoke}=await import('@tauri-apps/api/core');return invoke<T>(command,args);}
  const d=this.demo;
  switch(command){
   case 'network_monitor':{
    const tick=Math.floor(performance.now());
    return {interfaces:d.adapters.map((a,i)=>({id:a.id,name:a.name,received:String(tick*(i+1)*32),sent:String(tick*(i+1)*8)})),
     targets:['example.test:443 · Web'],events:[{time:Math.floor(Date.now()/1000),kind:'USB / IPv4',detail:'Ethernet 3 · 198.51.100.10'}],
     endpoints:[{pid:120,app:'example.exe',path:'Trực tiếp → LAN · Ethernet',evidence:'IP nguồn TCP 192.0.2.10',process:'example.exe',local:'192.0.2.10:51000',remote:'198.51.100.20:443',state:'ESTABLISHED'},
      {pid:121,app:'ms-teams.exe',process:'msedgewebview2.exe',path:'NetBridge → USB · Ethernet 3',evidence:'Tunnel #3 · example.test:443 · quy tắc #1 · IP nguồn 198.51.100.10',local:'127.0.0.1:52100',remote:'127.0.0.1:17890',state:'ESTABLISHED'},
      {pid:420,app:'netbridge.exe',path:'',evidence:'',process:'netbridge.exe',local:'127.0.0.1:17890',remote:null,state:'LISTEN'}],warnings:[]} as T;
   }
   case 'usb_health':return [{label:'Điện thoại',status:'ok',detail:'Thiết bị mô phỏng'},{label:'IPv4',status:'ok',detail:'Ethernet 3 · 198.51.100.10'},{label:'Gateway',status:'ok',detail:'198.51.100.1 · 1 ms'},{label:'Đích qua USB',status:'ok',detail:'example.test:443 · TCP thành công'}] as T;
   case 'get_snapshot':return clone(d) as T;
   case 'recover_config':if(args['expectedRevision']!==d.configRevision||!d.recoveryAvailable)throw new Error('Chưa thể phục hồi cấu hình.');d.configError=null;d.recoveryAvailable=false;d.configRevision='recovered';break;
   case 'export_settings':{
    const backup:SettingsBackup={format:'netbridge-settings',version:1,config:clone(d.config),preferences:{unit:d.unit}};
    const url=URL.createObjectURL(new Blob([JSON.stringify(backup,null,2)],{type:'application/json'}));const link=document.createElement('a');link.href=url;link.download='NetBridge-settings.json';link.click();setTimeout(()=>URL.revokeObjectURL(url),1000);return link.download as T;
   }
   case 'import_settings':return await new Promise<ImportPreview|null>((resolve,reject)=>{
    const input=document.createElement('input');input.type='file';input.accept='.json';input.setAttribute('aria-label','File sao lưu setting');input.hidden=true;document.body.append(input);
    input.oncancel=()=>{input.remove();resolve(null);};
    input.onchange=async()=>{try{const file=input.files?.[0];if(!file){resolve(null);return;}if(file.size>512*1024)throw new Error('File sao lưu vượt quá 512 KB.');const backup=JSON.parse(await file.text()) as SettingsBackup;
     if(backup.format!=='netbridge-settings'||backup.version!==1||!Array.isArray(backup.config?.rules)||!Array.isArray(backup.config?.check_targets)||!['Mbps','MB/s','MiB/s','Kbps','Gbps'].includes(backup.preferences?.unit))throw new Error('File sao lưu không hợp lệ.');
     resolve({backup,revision:d.configRevision,warnings:[]});}catch(e){reject(e);}finally{input.remove();}};input.click();
   }) as T;
   case 'restore_settings':{
    if(d.session.owned||d.job.active||d.configChanged||args['expectedRevision']!==d.configRevision)throw new Error('Chưa thể khôi phục: phiên đang chạy hoặc cấu hình đã thay đổi.');
    const backup=args['backup'] as SettingsBackup;d.config=clone(backup.config);d.session.endpoint='127.0.0.1:'+d.config.port;d.unit=backup.preferences.unit;d.configRevision=String((Number(d.configRevision)||0)+1);d.configError=null;d.recoveryAvailable=false;d.targets=[];d.speeds=[];return 'demo-recovery' as T;
   }
   case 'get_app_update':return clone(this.appUpdate) as T;
   case 'check_app_update':{
    if(new URLSearchParams(location.search).get('scenario')==='update-error'){
     this.appUpdate={...this.appUpdate,status:'error',error:'Không kết nối được máy chủ cập nhật. Thử lại sau.'};throw new Error(this.appUpdate.error!);
    }
    this.appUpdate={status:'available',version:packageInfo.version.split('.').slice(0,2).map((part,index)=>index===1?String(Number(part)+1):part).join('.')+'.0',notes:'Cải thiện hiệu năng và trải nghiệm sử dụng. Dữ liệu mô phỏng.',downloaded:0,total:null,error:null};break;
   }
   case 'install_app_update':{
    if(args['expectedVersion']!==this.appUpdate.version)throw new Error('Phiên bản đã thay đổi. Kiểm tra lại.');
    this.appUpdate={...this.appUpdate,status:'demo-installed'};break;
   }
   case 'save_config':{
    const cfg=args['config'] as Config;
    if(args['expectedRevision']!==d.configRevision||d.configChanged)throw new Error('File cấu hình đã thay đổi ngoài ứng dụng. Tải cấu hình mới trước khi lưu.');
    if(cfg.rules.some(r=>!r.pattern.trim()||/\s/.test(r.pattern)))throw new Error('Domain hoặc CIDR chưa hợp lệ.');
    d.config=clone(cfg);d.configRevision=String(Number(d.configRevision)+1);d.targets=[];d.logs.push('Đã lưu cấu hình mô phỏng.');break;
   }
   case 'reload_config':d.configChanged=false;d.configError=null;d.recoveryAvailable=false;d.targets=[];break;
   case 'start_routing':d.session={kind:'active',owned:true,label:'Đang phân luồng',detail:'Phiên mô phỏng. Không thay đổi proxy Windows.',endpoint:d.session.endpoint};break;
   case 'stop_routing':d.session={...d.session,kind:'inactive',owned:false,label:'Đang dùng mạng Windows',detail:'Phiên mô phỏng đã tắt.'};break;
   case 'get_devices':{
    const scenario=new URLSearchParams(location.search).get('scenario');
    const first={serial:'DEMO-USB-001',model:'Android_USB',state:'device',usb:true,adapterIds:scenario==='multiple-usb'?[]:['demo-usb']};
    return (scenario==='paired-usb'?[first,{...first,serial:'DEMO-USB-002',model:'Android_B',adapterIds:['demo-second']}]:[first]) as T;
   }
   case 'get_adb':return {version:'37.0.1',path:'ADB tích hợp · dữ liệu mô phỏng',bundled:true} as T;
   case 'diagnostic_details':return {traffic:{active:0,rejected:0,rows:[]},backups:{count:0,bytes:0,latest:null},watchdog:null} as T;
   case 'valid_rule':{
    const pattern=String(args['pattern']);
    if(pattern.includes('/')){
     const cidr=/^(\d+(?:\.\d+){3})\/\+?(\d+)$/.exec(pattern);
     return (!!cidr && Number(cidr[2])<=32 && cidr[1].split('.').every(v=>String(Number(v))===v && Number(v)<=255)) as T;
    }
    const host=pattern.replace(/^\*\./,'').replace(/\.+$/,'');
    return (host.length<=253 && host.split('.').every(label=>/^[a-z0-9_-]{1,63}$/i.test(label))) as T;
   }
   case 'try_rule':{
    const cfg=args['config'] as Config;const host=String(args['host']).trim().toLowerCase();
    if(!host||/\s/.test(host))throw new Error('Nhập hostname hợp lệ.');
    const i=cfg.rules.findIndex(r=>r.pattern===host||(r.pattern.startsWith('*.')&&(host===r.pattern.slice(2)||host.endsWith(r.pattern.slice(1)))));
    return {route:i<0?'system':cfg.rules[i].route,ruleIndex:i<0?null:i} as T;
   }
   case 'set_unit':d.unit=args['unit'] as Unit;break;
   case 'cancel_job':
    if(this.timer)clearTimeout(this.timer);
    d.job={...d.job,active:false,cancelling:true,phase:'Đã dừng.'};break;
   case 'check_targets':case 'measure_network':case 'tether':case 'adb_update':case 'connect_usb_and_route':{
    if(d.job.active)throw new Error('Có tác vụ đang chạy.');
    const id=d.job.id+1;
    d.job={id,kind:command==='measure_network'?'speed':command==='check_targets'?'check':command==='tether'||command==='connect_usb_and_route'?'usb':'adb',phase:command==='measure_network'?'Đang đo độ trễ HTTP…':command==='connect_usb_and_route'?'Đang chờ USB và bật phân luồng…':'Đang kiểm tra…',active:true,cancelling:false,error:null};
    this.timer=setTimeout(()=>{
     if(command==='check_targets')d.targets=d.config.check_targets.map(t=>{const failure=new URLSearchParams(location.search).get('scenario')==='windows-route-failure';return {...t,route:d.config.rules.find(r=>r.pattern===t.host)?.route??'system',ok:true,detail:'TCP đạt · dữ liệu mô phỏng',checkedAt:String(Math.floor(Date.now()/1000)),windows:{ok:!failure,sourceIp:failure?null:'192.0.2.10',predictedSourceIp:failure?'198.51.100.10':'192.0.2.10',adapter:failure?'Ethernet 3':'Ethernet',remoteIp:'192.0.2.20',detail:failure?'TCP timed out':'TCP đạt'},selected:{ok:true,sourceIp:'192.0.2.10',predictedSourceIp:'192.0.2.10',adapter:'Ethernet',remoteIp:'192.0.2.20',detail:'TCP đạt'}};});
     if(command==='connect_usb_and_route'){
      if(new URLSearchParams(location.search).get('scenario')==='usb-connect-failure'){d.job={...d.job,active:false,error:'Không bật được phân luồng. Đã hoàn tác Ethernet USB vừa bật.',phase:'Kết nối chưa hoàn tất.'};return;}
      const selected=args['confirmedAdapter'] as [string,string]|null;
      if(selected && selected[0]!==d.config.usb_id){d.config.usb_id=selected[0];d.configRevision=String(Number(d.configRevision)+1);}
      d.session={kind:'active',owned:true,label:'Đang phân luồng',detail:'USB và proxy đã sẵn sàng · dữ liệu mô phỏng.',endpoint:d.session.endpoint};
     }
     if(command==='measure_network')d.speeds=d.adapters.filter(a=>args['adapterId']==='all'||args['adapterId']===a.id).map((a,i)=>({
      adapter:a.name,sourceIp:a.ip,downloadMbps:i?76.4:154.8,uploadMbps:i?18.6:38.2,latencyMs:i?92:34.6,
      grade:i?'Tốt':'Rất tốt',explanation:i?'Chỉ số giới hạn: download, upload, độ trễ HTTP.':'Cả ba chỉ số đều đạt ngưỡng rất tốt.',errors:[]}));
     if(command==='adb_update')d.adbLatest={version:'37.0.1'};
     d.job={...d.job,active:false,phase:'Đã hoàn tất.'};d.logs.push('Hoàn tất tác vụ mô phỏng.');
    },650);return id as T;
   }
   case 'exit_app':return undefined as T;
   default:throw new Error('Unknown command: '+command);
  }return undefined as T;
 }
 async listen(refresh:()=>void,error:(message:string)=>void):Promise<()=>void>{
  if(!this.native)return ()=>{};
  const {listen}=await import('@tauri-apps/api/event');
  const a=await listen('netbridge://state',refresh);
  const b=await listen<string>('netbridge://error',e=>error(e.payload));
  return ()=>{a();b();};
 }
}

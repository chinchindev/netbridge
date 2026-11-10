import {Injectable} from '@angular/core';
import {AppUpdate,Snapshot,Config,Unit,targets} from './model';
import packageInfo from '../../package.json';
const clone=<T>(value:T):T=>structuredClone(value);
@Injectable({providedIn:'root'})
export class Gateway {
 readonly native='__TAURI_INTERNALS__' in window;
 private demo:Snapshot={
  mode:'demo',version:packageInfo.version,configRevision:'0',configChanged:false,
  config:{port:17890,lan_id:'demo-lan',usb_id:'demo-usb',adb_path:'auto',rules:[
   {pattern:'git.etc.vn',route:'lan'},{pattern:'10.3.6.5',route:'lan'},
   {pattern:'*.onorca.dev',route:'usb'},{pattern:'103.122.82.124',route:'usb'}]},
  adapters:[
   {id:'demo-lan',name:'Ethernet',description:'LAN nội bộ',ip:'10.3.80.189',dns:['10.3.0.1'],index:1,if_type:6},
   {id:'demo-usb',name:'Ethernet 3',description:'USB · M9200',ip:'192.168.183.100',dns:['192.168.183.12'],index:2,if_type:6}],
  session:{kind:'inactive',owned:false,label:'Sẵn sàng kết nối',detail:'Bật phân luồng để chọn đường mạng theo từng đích.',endpoint:'127.0.0.1:17890'},
  job:{id:0,kind:'',phase:'',active:false,cancelling:false,error:null},
  targets:[],speeds:[],logs:['NetBridge sẵn sàng. Dữ liệu trong trình duyệt là mô phỏng.'],unit:'Mbps',adbLatest:null
 };
 private timer:ReturnType<typeof setTimeout>|undefined;
 private appUpdate:AppUpdate={status:'',version:null,notes:null,downloaded:0,total:null,error:null};
 constructor(){if(!this.native&&new URLSearchParams(location.search).get('scenario')==='external-config')this.demo.configChanged=true;}
 async invoke<T>(command:string,args:Record<string,unknown>={}):Promise<T>{
  if(this.native){const {invoke}=await import('@tauri-apps/api/core');return invoke<T>(command,args);}
  const d=this.demo;
  switch(command){
   case 'get_snapshot':return clone(d) as T;
   case 'get_app_update':return clone(this.appUpdate) as T;
   case 'check_app_update':{
    if(new URLSearchParams(location.search).get('scenario')==='update-error'){
     this.appUpdate={...this.appUpdate,status:'error',error:'Không kết nối được máy chủ cập nhật. Thử lại sau.'};throw new Error(this.appUpdate.error!);
    }
    this.appUpdate={status:'available',version:'0.4.0',notes:'Cải thiện hiệu năng và trải nghiệm sử dụng. Dữ liệu mô phỏng.',downloaded:0,total:null,error:null};break;
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
   case 'reload_config':d.configChanged=false;d.targets=[];break;
   case 'start_routing':d.session={kind:'active',owned:true,label:'Đang phân luồng',detail:'Phiên mô phỏng. Không thay đổi proxy Windows.',endpoint:d.session.endpoint};break;
   case 'stop_routing':d.session={...d.session,kind:'inactive',owned:false,label:'Đang dùng mạng Windows',detail:'Phiên mô phỏng đã tắt.'};break;
   case 'get_devices':return [{serial:'2870003386',model:'M9200',state:'device',usb:true}] as T;
   case 'get_adb':return {version:'37.0.1',path:'ADB tích hợp · dữ liệu mô phỏng',bundled:true} as T;
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
   case 'check_targets':case 'measure_network':case 'tether':case 'adb_update':{
    if(d.job.active)throw new Error('Có tác vụ đang chạy.');
    const id=d.job.id+1;
    d.job={id,kind:command==='measure_network'?'speed':command==='check_targets'?'check':command==='tether'?'usb':'adb',phase:command==='measure_network'?'Đang đo độ trễ HTTP…':'Đang kiểm tra…',active:true,cancelling:false,error:null};
    this.timer=setTimeout(()=>{
     if(command==='check_targets')d.targets=targets.map(t=>({...t,ok:true,detail:'TCP đạt · dữ liệu mô phỏng',checkedAt:String(Math.floor(Date.now()/1000))}));
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

import {Component,computed,inject,signal,OnInit,OnDestroy,HostListener} from '@angular/core';
import {CommonModule} from '@angular/common';
import {FormsModule} from '@angular/forms';
import {Gateway} from './gateway';
import {Icon} from './icon';
import {Select,SelectOption} from './select';
import {remapBackup,restoreAdapterOptions,restoreWarnings} from './restore-mapping';
import {Adapter,AppUpdate,Config,Device,ImportPreview,Page,Route,Snapshot,Unit,convert} from './model';
@Component({selector:'app-root',standalone:true,imports:[CommonModule,FormsModule,Icon,Select],templateUrl:'./app.html'})
export class App implements OnInit,OnDestroy{
 readonly api=inject(Gateway);
 readonly state=signal<Snapshot|null>(null);
 readonly page=signal<Page>('overview');
 readonly error=signal('');readonly notice=signal('');readonly busy=signal(false);readonly dirty=signal(false);
 readonly devices=signal<Device[]>([]);readonly devicesLoading=signal(false);
 readonly adb=signal<{version:string;path:string}|null>(null);
 readonly diagnostics=signal(false);readonly criteria=signal(false);readonly stopConfirm=signal(false);readonly exitConfirm=signal(false);
 readonly updateConfirm=signal(false);
 readonly recoveryConfirm=signal(false);
 readonly restorePreview=signal<ImportPreview|null>(null);
 readonly restoreLan=signal('');readonly restoreUsb=signal('');readonly restoreBundledAdb=signal(true);
 readonly appUpdate=signal<AppUpdate>({status:'',version:null,notes:null,downloaded:0,total:null,error:null});
 readonly unit=signal<Unit>('Mbps');readonly units:Unit[]=['Mbps','MB/s','MiB/s','Kbps','Gbps'];
 readonly unitOptions:SelectOption[]=this.units.map(value=>({value,label:value}));
 readonly routeOptions:SelectOption[]=[{value:'lan',label:'LAN nội bộ'},{value:'usb',label:'Internet USB'},{value:'system',label:'Theo Windows'}];
 readonly adapterOptions=computed<SelectOption[]>(()=>[{value:'',label:'Chọn card mạng'},...(this.state()?.adapters??[]).map(a=>({value:a.id,label:a.name+' · '+a.ip}))]);
 readonly measureOptions=computed<SelectOption[]>(()=>[{value:'all',label:'Tất cả adapter đang hoạt động'},...this.adapterOptions().slice(1)]);
 readonly links:{id:Page;label:string;icon:string;caption:string}[]=[
  {id:'overview',label:'Tổng quan',icon:'grid',caption:'Mọi kết nối, đúng đường.'},
  {id:'connections',label:'Kết nối',icon:'network',caption:'Chuẩn bị các đường mạng của bạn.'},
  {id:'rules',label:'Quy tắc',icon:'rules',caption:'Chủ động chọn đường đi cho từng đích.'},
  {id:'speed',label:'Đo mạng',icon:'gauge',caption:'Hiểu hiệu năng của từng đường truyền.'},
  {id:'settings',label:'Cài đặt',icon:'settings',caption:'Công cụ luôn sẵn sàng khi bạn cần.'}];
 readonly targetDefinitions=computed(()=>this.state()?.config.check_targets??[]);
 readonly currentPage=computed(()=>this.links.find(p=>p.id===this.page())!);
 readonly active=computed(()=>!!this.state()?.session.owned);
 readonly canWrite=computed(()=>this.state()?.mode!=='preview'&&!this.state()?.configError);
 readonly passed=computed(()=>this.state()?.targets.filter(t=>t.ok).length??0);
 draft:Config={port:17890,lan_id:'',usb_id:'',adb_path:'auto',rules:[],check_targets:[]};
 checkName='';checkHost='';checkPort=443;checkError='';
 private draftRevision='';
 rulePattern='';ruleRoute:Route='lan';ruleEdit=-1;ruleError='';
 probeHost='';probeResult:{route:Route;ruleIndex:number|null}|null=null;speedAdapter='all';
 private timer:ReturnType<typeof setInterval>|undefined;private unsubscribe:()=>void=()=>{};
 private refreshing=false;private toastTimer:ReturnType<typeof setTimeout>|undefined;
 async ngOnInit(){await this.refresh();this.unsubscribe=await this.api.listen(()=>{void this.refresh();},m=>this.error.set(m));this.timer=setInterval(()=>void this.refresh(),1200);}
 ngOnDestroy(){if(this.timer)clearInterval(this.timer);if(this.toastTimer)clearTimeout(this.toastTimer);this.unsubscribe();}
 private focusReturn:HTMLElement|null=null;
 modalOpen(){return this.criteria()||this.diagnostics()||this.stopConfirm()||this.exitConfirm()||this.updateConfirm()||this.recoveryConfirm()||!!this.restorePreview();}
 openDialog(kind:'criteria'|'diagnostics'|'stop'|'exit'|'update'|'recovery'){
  this.focusReturn=document.activeElement as HTMLElement;
  this.criteria.set(kind==='criteria');this.diagnostics.set(kind==='diagnostics');this.stopConfirm.set(kind==='stop');this.exitConfirm.set(kind==='exit');
  this.updateConfirm.set(kind==='update');
  this.recoveryConfirm.set(kind==='recovery');
  setTimeout(()=>document.querySelector<HTMLElement>('[role=dialog] button')?.focus(),0);
 }
 @HostListener('document:keydown.escape') escape(){
  this.criteria.set(false);this.diagnostics.set(false);this.stopConfirm.set(false);this.exitConfirm.set(false);
  this.updateConfirm.set(false);
  this.recoveryConfirm.set(false);
  this.restorePreview.set(null);
  const previous=this.focusReturn;setTimeout(()=>previous?.isConnected&&previous.focus(),0);
 }
 @HostListener('document:keydown',['$event']) trapTab(event:KeyboardEvent){
  if(event.key!=='Tab'||!this.modalOpen())return;
  const items=Array.from(document.querySelectorAll<HTMLElement>('[role=dialog] button:not([disabled]),[role=dialog] input,[role=dialog] select,[role=dialog] a[href]')).filter(el=>el.offsetParent!==null);
  const first=items[0],last=items[items.length-1];
  if(event.shiftKey&&document.activeElement===first){event.preventDefault();last?.focus();}
  else if(!event.shiftKey&&document.activeElement===last){event.preventDefault();first?.focus();}
 }
 async refresh(){
  if(this.refreshing)return;this.refreshing=true;
  try{
   const previous=this.state();const [next,appUpdate]=await Promise.all([this.api.invoke<Snapshot>('get_snapshot'),this.api.invoke<AppUpdate>('get_app_update')]);
   this.appUpdate.set(appUpdate);
   if(!this.dirty()&&!this.busy()){this.draft=structuredClone(next.config);this.draftRevision=next.configRevision;}
   this.state.set(next);if(!previous)this.unit.set(next.unit);
   if(previous?.job.active&&!next.job.active){
    if(next.job.error)this.error.set(next.job.error);else this.toast(next.job.cancelling?'Đã dừng tác vụ.':next.job.phase);
    if(next.job.kind==='usb')void this.loadDevices();if(next.job.kind==='adb')void this.loadAdb();
   }
  }catch(e){this.error.set(this.message(e));}finally{this.refreshing=false;}
 }
 go(page:Page){this.page.set(page);document.getElementById('main-content')?.scrollTo({top:0});if(page==='connections'&&!this.devices().length)void this.loadDevices();if(page==='settings'&&!this.adb())void this.loadAdb();}
 changed(){this.dirty.set(true);}
 sameAdapter(){return !!this.draft.lan_id&&this.draft.lan_id===this.draft.usb_id;}
 discard(){if(this.state()){this.draft=structuredClone(this.state()!.config);this.draftRevision=this.state()!.configRevision;}this.dirty.set(false);this.ruleEdit=-1;this.rulePattern='';this.ruleError='';}
 async reloadConfig(){if(await this.operation('reload_config',{},'Đã tải cấu hình mới.'))this.discard();}
 message(e:unknown){return e instanceof Error?e.message:typeof e==='string'?e:'Không thể hoàn tất tác vụ. Xem chẩn đoán để biết thêm.';}
 toast(message:string){this.notice.set(message);if(this.toastTimer)clearTimeout(this.toastTimer);this.toastTimer=setTimeout(()=>this.notice.set(''),4500);}
 async operation(command:string,args:Record<string,unknown>={},success=''){
  if(this.busy())return false;this.busy.set(true);this.error.set('');
  try{await this.api.invoke(command,args);if(success)this.toast(success);return true;}
  catch(e){this.error.set(this.message(e));return false;}
  finally{this.busy.set(false);await this.refresh();}
 }
 async save(){
  if(!this.canWrite())return false;this.error.set('');this.busy.set(true);
  try{await this.api.invoke('save_config',{config:structuredClone(this.draft),expectedRevision:this.draftRevision});this.dirty.set(false);this.toast('Đã lưu. Quy tắc áp dụng cho kết nối mới.');return true;}
  catch(e){this.error.set(this.message(e));return false;}finally{this.busy.set(false);await this.refresh();}
 }
 async toggle(){if(this.active()){this.openDialog('stop');return;}if(this.dirty()&&!await this.save())return;await this.operation('start_routing',{},'Phân luồng đã bật. Đóng cửa sổ sẽ giữ phiên chạy ở khay hệ thống.');}
 async stop(){this.stopConfirm.set(false);await this.operation('stop_routing',{},'Đã tắt phiên. Kết nối trở về cấu hình Windows.');}
 adapter(route:'lan'|'usb'):Adapter|undefined{const cfg=this.state()?.config;return this.state()?.adapters.find(a=>a.id===(route==='lan'?cfg?.lan_id:cfg?.usb_id));}
 routeName(route:Route){return route==='lan'?'LAN nội bộ':route==='usb'?'Internet USB':'Theo Windows';}
 routeShort(route:Route){return route==='lan'?'LAN':route==='usb'?'USB':'Windows';}
 target(host:string,port:number){return this.state()?.targets.find(t=>t.host===host&&t.port===port);}
 time(value:string){return new Date(Number(value)*1000).toLocaleTimeString('vi-VN',{hour:'2-digit',minute:'2-digit'});}
 ready(){const cfg=this.state()?.config;return !!cfg&&cfg.rules.length>0&&cfg.rules.every(rule=>rule.route==='system'||!!this.adapter(rule.route));}
 async loadDevices(){this.devicesLoading.set(true);try{this.devices.set(await this.api.invoke<Device[]>('get_devices'));}catch(e){this.error.set(this.message(e));}finally{this.devicesLoading.set(false);}}
 async loadAdb(){try{this.adb.set(await this.api.invoke('get_adb'));}catch(e){this.error.set(this.message(e));}}
 async tether(device:Device,connect:boolean){await this.operation('tether',{serial:device.serial,connect});}
 async connectAndRoute(device:Device){if(this.dirty())return;await this.operation('connect_usb_and_route',{serial:device.serial,expectedRevision:this.state()?.configRevision});}
 async recoverConfig(){this.recoveryConfirm.set(false);if(await this.operation('recover_config',{expectedRevision:this.state()?.configRevision},'Đã phục hồi cấu hình. File lỗi đã được giữ riêng.'))this.discard();}
 async check(){await this.operation('check_targets');}
 addCheckTarget(){
  const name=this.checkName.trim(),host=this.checkHost.trim().toLowerCase(),port=Number(this.checkPort);
  if(!name||name.length>64||!host||!/^[a-z0-9.-]+$/i.test(host)||!Number.isInteger(port)||port<1||port>65535){this.checkError='Nhập tên, hostname hoặc IPv4 và cổng từ 1 đến 65535.';return;}
  if(this.draft.check_targets.length>=4){this.checkError='Có thể lưu tối đa 4 đích kiểm tra.';return;}
  if(this.draft.check_targets.some(t=>t.host===host&&t.port===port)){this.checkError='Đích và cổng này đã có trong danh sách.';return;}
  this.draft.check_targets.push({name,host,port});this.checkName='';this.checkHost='';this.checkPort=443;this.checkError='';this.changed();
 }
 removeCheckTarget(index:number){this.draft.check_targets.splice(index,1);this.changed();}
 async exportSettings(){
  if(this.busy()||this.dirty())return;this.busy.set(true);this.error.set('');
  try{const file=await this.api.invoke<string|null>('export_settings');if(file)this.toast('Đã xuất file sao lưu setting.');}
  catch(e){this.error.set(this.message(e));}finally{this.busy.set(false);await this.refresh();}
 }
 async importSettings(){
  if(this.busy()||this.dirty()||this.active())return;this.focusReturn=document.activeElement as HTMLElement;this.busy.set(true);this.error.set('');
  try{const preview=await this.api.invoke<ImportPreview|null>('import_settings');if(preview){this.restoreLan.set(preview.backup.config.lan_id);this.restoreUsb.set(preview.backup.config.usb_id);this.restoreBundledAdb.set(preview.backup.config.adb_path==='auto');this.restorePreview.set(preview);setTimeout(()=>document.querySelector<HTMLElement>('[role=dialog] button')?.focus(),0);}}
  catch(e){this.error.set(this.message(e));}finally{this.busy.set(false);await this.refresh();}
 }
 async restoreSettings(){
  const preview=this.restorePreview();if(!preview)return;const backup=remapBackup(preview.backup,this.restoreLan(),this.restoreUsb(),this.restoreBundledAdb());this.restorePreview.set(null);
  if(await this.operation('restore_settings',{backup,expectedRevision:preview.revision},'Đã khôi phục setting và giữ bản sao cấu hình trước đó.')){this.unit.set(backup.preferences.unit);this.discard();}
 }
 restoreOptions(role:'lan'|'usb'){const preview=this.restorePreview();return preview?restoreAdapterOptions(this.state()?.adapters??[],role==='lan'?preview.backup.config.lan_id:preview.backup.config.usb_id,role==='lan'?this.restoreLan():this.restoreUsb()):[];}
 restoreMessages(){const preview=this.restorePreview();return preview?restoreWarnings(remapBackup(preview.backup,this.restoreLan(),this.restoreUsb(),this.restoreBundledAdb()),this.state()?.adapters??[]):[];}
 editRule(index:number){this.ruleEdit=index;this.rulePattern=this.draft.rules[index].pattern;this.ruleRoute=this.draft.rules[index].route;this.ruleError='';}
 addRule(){
  const pattern=this.rulePattern.trim().toLowerCase();
  if(!pattern||/\s/.test(pattern)){this.ruleError='Nhập domain, IPv4 hoặc CIDR hợp lệ, không có khoảng trắng.';return;}
  if(this.draft.rules.some((rule,i)=>rule.pattern===pattern&&i!==this.ruleEdit)){this.ruleError='Đích này đã có quy tắc. Chọn dòng hiện có để sửa.';return;}
  const rule={pattern,route:this.ruleRoute};if(this.ruleEdit>=0)this.draft.rules[this.ruleEdit]=rule;else this.draft.rules.push(rule);
  this.rulePattern='';this.ruleEdit=-1;this.ruleError='';this.changed();
 }
 removeRule(index:number){this.draft.rules.splice(index,1);this.ruleEdit=-1;this.rulePattern='';this.changed();}
 moveRule(index:number,direction:number){const target=index+direction;if(target<0||target>=this.draft.rules.length)return;[this.draft.rules[index],this.draft.rules[target]]=[this.draft.rules[target],this.draft.rules[index]];this.ruleEdit=-1;this.rulePattern='';this.changed();}
 async probe(){this.error.set('');try{this.probeResult=await this.api.invoke('try_rule',{config:structuredClone(this.draft),host:this.probeHost});}catch(e){this.probeResult=null;this.error.set(this.message(e));}}
 async measure(){await this.operation('measure_network',{adapterId:this.speedAdapter});}
 async cancel(){await this.operation('cancel_job',{jobId:this.state()?.job.id});}
 async changeUnit(unit:Unit){this.unit.set(unit);if(this.state()?.mode==='preview')return;try{await this.api.invoke('set_unit',{unit});}catch(e){this.error.set(this.message(e));}}
 rate(value:number|null){const n=convert(value,this.unit());return n===null?'—':new Intl.NumberFormat('vi-VN',{minimumFractionDigits:2,maximumFractionDigits:2}).format(n);}
 number(value:number|null){return value===null?'—':new Intl.NumberFormat('vi-VN',{maximumFractionDigits:1}).format(value);}
 bar(value:number|null){const max=Math.max(1,...(this.state()?.speeds.map(s=>s.downloadMbps??0)??[]));return Math.max(value===null?0:2,(value??0)/max*100);}
 gradeClass(label:string){return label==='Rất tốt'?'excellent':label==='Tốt'?'good':label==='Trung bình'?'fair':label==='Yếu'?'poor':'neutral';}
 isNewAdb(){const latest=this.state()?.adbLatest?.version;const current=this.adb()?.version;if(!latest||!current)return false;return latest.split('.').map(Number).some((n,i,all)=>all.slice(0,i).every((v,j)=>v===Number(current.split('.')[j]))&&n>Number(current.split('.')[i]));}
 async updateAdb(install:boolean){await this.operation('adb_update',{install});}
 async checkAppUpdate(){await this.operation('check_app_update');}
 async installAppUpdate(){this.updateConfirm.set(false);await this.operation('install_app_update',{expectedVersion:this.appUpdate().version});}
 updatePercent(){const update=this.appUpdate();return update.total?Math.min(100,Math.round(update.downloaded/update.total*100)):0;}
 async exit(){this.exitConfirm.set(false);await this.operation('exit_app');}
}

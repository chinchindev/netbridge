import {Component,computed,inject,OnDestroy,OnInit,signal} from '@angular/core';
import {DatePipe} from '@angular/common';
import {FormsModule} from '@angular/forms';
import {Gateway} from './gateway';
import {Select} from './select';

interface Counters {id:string;name:string;received:string;sent:string}
interface Endpoint {pid:number;process:string;local:string;remote:string|null;state:string;app:string;path:string;evidence:string}
interface Snapshot {interfaces:Counters[];endpoints:Endpoint[];warnings:string[];events:Incident[];targets:string[]}
interface Incident {time:number;kind:string;detail:string}
interface HealthStep {label:string;status:'ok'|'fail'|'unknown';detail:string}
interface Rate {down:number;up:number}
interface History {sample:Counters;time:number;points:Rate[]}

@Component({selector:'nb-network-monitor',standalone:true,imports:[FormsModule,Select,DatePipe],templateUrl:'./network-monitor.html'})
export class NetworkMonitor implements OnInit,OnDestroy {
 private readonly api=inject(Gateway);
 readonly snapshot=signal<Snapshot|null>(null);
 readonly histories=signal(new Map<string,History>());
 readonly adapterId=signal('');
 readonly filter=signal('');
 readonly listeners=signal(false);
 readonly paused=signal(false);
 readonly error=signal('');
 readonly limit=signal(50);
 readonly updated=signal('');
 readonly options=computed(()=>this.snapshot()?.interfaces.map(a=>({label:a.name,value:a.id}))??[]);
 readonly current=computed(()=>this.histories().get(this.adapterId()));
 readonly rate=computed(()=>this.current()?.points.at(-1));
 readonly lines=computed(()=>{
  const points=this.current()?.points??[];
  const peak=Math.max(1,...points.map(p=>Math.max(p.down,p.up)));
  const line=(field:keyof Rate)=>points.map((p,i)=>((30-points.length+i)*300/29).toFixed(1)+','+(60-p[field]/peak*55).toFixed(1)).join(' ');
  return {down:line('down'),up:line('up'),peak};
 });
 readonly appFilter=signal('');
 readonly apps=computed(()=>{
  const apps=new Set(this.snapshot()?.endpoints.map(row=>row.app).filter(Boolean));
  if(this.appFilter())apps.add(this.appFilter());
  return [{value:'',label:'Tất cả ứng dụng'},...Array.from(apps).sort().map(app=>({value:app,label:app}))];
 });
 readonly healthTarget=signal('');
 readonly healthOptions=computed(()=>(this.snapshot()?.targets??[]).map(t=>({value:t,label:t})));
 readonly health=signal<HealthStep[]>([]);
 readonly checking=signal(false);
 readonly checked=signal('');
 readonly healthError=signal('');
 readonly expanded=signal('');
 readonly showAllEvents=signal(false);
 rowKey(row:Endpoint){return [row.pid,row.local,row.remote].join('|');}
 setHealthTarget(value:string){this.healthTarget.set(value);this.health.set([]);this.checked.set('');this.healthError.set('');}
 async checkUsb(){
  if(this.checking())return;
  this.checking.set(true);this.healthError.set('');this.health.set([]);this.checked.set('');
  try{
   const result=await this.api.invoke<HealthStep[]>('usb_health',{target:this.healthTarget()||null});
   if(!this.disposed){this.health.set(result);this.checked.set(new Date().toLocaleTimeString('vi-VN'));void this.sample();}
  }catch(e){if(!this.disposed)this.healthError.set(String(e));}finally{this.checking.set(false);}
 }
 readonly rows=computed(()=>{
  const q=this.filter().trim().toLowerCase();
  return (this.snapshot()?.endpoints??[]).filter(e=>(e.state==='LISTEN')===this.listeners() &&
   (!this.appFilter()||e.app===this.appFilter()) && (!q || [e.process,e.app,e.path,e.evidence,String(e.pid),e.local,e.remote??'',e.state].some(v=>v.toLowerCase().includes(q))));
 });
 private loading=false;
 private disposed=false;
 private timer:ReturnType<typeof setInterval>|undefined;
 private generation=0;
 ngOnInit(){void this.sample();this.timer=setInterval(()=>void this.sample(),2000);}
 ngOnDestroy(){this.disposed=true;clearInterval(this.timer);}
 togglePause(){
  this.generation++;this.paused.update(value=>!value);
  if(!this.paused()){this.histories.set(new Map());void this.sample();}
 }
 setTab(listeners:boolean){this.listeners.set(listeners);this.limit.set(50);}
 setFilter(q:string){this.filter.set(q);this.limit.set(50);}
 async sample(){
  if(this.loading||this.paused())return;
  this.loading=true;
  const generation=this.generation;
  try{
   const data=await this.api.invoke<Snapshot>('network_monitor');
   if(this.disposed||generation!==this.generation)return;
   const now=performance.now(), previous=this.histories(), next=new Map<string,History>();
   for(const a of data.interfaces){
    const old=previous.get(a.id);let points:Rate[]=[];
    if(old && now>old.time && now-old.time<10000){
     const down=BigInt(a.received)-BigInt(old.sample.received),up=BigInt(a.sent)-BigInt(old.sample.sent);
     if(down>=0n && up>=0n){
      const seconds=(now-old.time)/1000;
      points=[...old.points.slice(-29),{down:Number(down)/seconds,up:Number(up)/seconds}];
     }
    }
    next.set(a.id,{sample:a,time:now,points});
   }
   this.histories.set(next);
   if(!next.has(this.adapterId()))this.adapterId.set(data.interfaces[0]?.id??'');
   const target=data.targets.includes(this.healthTarget())?this.healthTarget():(data.targets[0]??'');
   if(target!==this.healthTarget())this.setHealthTarget(target);
   this.snapshot.set(data);this.error.set('');this.updated.set(new Date().toLocaleTimeString('vi-VN'));
  }catch(e){if(!this.disposed && generation===this.generation)this.error.set(String(e));}
  finally{this.loading=false;}
 }
 bytes(value:number|string|undefined){
  if(value===undefined)return '—';
  const n=Number(value),units=['B','KiB','MiB','GiB','TiB','PiB'];
  const i=Math.min(units.length-1,Math.max(0,Math.floor(Math.log(Math.max(n,1))/Math.log(1024))));
  return (n/1024**i).toLocaleString('vi-VN',{maximumFractionDigits:1})+' '+units[i];
 }
}

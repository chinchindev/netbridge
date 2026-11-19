import {Component,computed,inject,OnDestroy,OnInit,signal} from '@angular/core';
import {FormsModule} from '@angular/forms';
import {Gateway} from './gateway';
import {Select} from './select';

interface Counters {id:string;name:string;received:string;sent:string}
interface Endpoint {pid:number;process:string;local:string;remote:string|null;state:string}
interface Snapshot {interfaces:Counters[];endpoints:Endpoint[];warnings:string[]}
interface Rate {down:number;up:number}
interface History {sample:Counters;time:number;points:Rate[]}

@Component({selector:'nb-network-monitor',standalone:true,imports:[FormsModule,Select],templateUrl:'./network-monitor.html'})
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
 readonly rows=computed(()=>{
  const q=this.filter().trim().toLowerCase();
  return (this.snapshot()?.endpoints??[]).filter(e=>(e.state==='LISTEN')===this.listeners() &&
   (!q || [e.process,String(e.pid),e.local,e.remote??'',e.state].some(v=>v.toLowerCase().includes(q))));
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

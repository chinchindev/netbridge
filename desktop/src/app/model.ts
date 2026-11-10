export type Route='lan'|'usb'|'system';
export type Unit='Mbps'|'MB/s'|'MiB/s'|'Kbps'|'Gbps';
export type Page='overview'|'connections'|'rules'|'speed'|'settings';
export interface AppUpdate{status:string;version:string|null;notes:string|null;downloaded:number;total:number|null;error:string|null}
export interface Rule{pattern:string;route:Route}
export interface Config{port:number;lan_id:string;usb_id:string;adb_path:string;rules:Rule[]}
export interface Adapter{id:string;name:string;description:string;ip:string;dns:string[];index:number;if_type:number}
export interface Device{serial:string;model:string;state:string;usb:boolean}
export interface Target{name:string;host:string;port:number;route:Route;ok:boolean;detail:string;checkedAt:string}
export interface Speed{adapter:string;sourceIp:string;downloadMbps:number|null;uploadMbps:number|null;latencyMs:number|null;grade:string;explanation:string;errors:string[]}
export interface Snapshot{
 mode:'live'|'preview'|'demo';version:string;config:Config;configRevision:string;configChanged:boolean;exitAllowed?:boolean;adapters:Adapter[];
 session:{kind:string;owned:boolean;label:string;detail:string;endpoint:string};
 job:{id:number;kind:string;phase:string;active:boolean;cancelling:boolean;error:string|null};
 targets:Target[];speeds:Speed[];logs:string[];unit:Unit;adbLatest?:{version:string}|null;
}
export const targets=[
 {name:'Git ETC',host:'git.etc.vn',port:443,route:'lan' as Route,initial:'G'},
 {name:'SRS',host:'10.3.6.5',port:12443,route:'lan' as Route,initial:'S'},
 {name:'Orca',host:'www.onorca.dev',port:443,route:'usb' as Route,initial:'O'},
 {name:'MQTT',host:'103.122.82.124',port:1883,route:'usb' as Route,initial:'M'}
];
export function convert(value:number|null,unit:Unit):number|null{
 if(value===null||!Number.isFinite(value)||value<0)return null;
 return value*({Mbps:1,'MB/s':1/8,'MiB/s':1e6/8/1048576,Kbps:1000,Gbps:0.001}[unit]);
}

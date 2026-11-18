export type Route='lan'|'usb'|'system';
export type Unit='Mbps'|'MB/s'|'MiB/s'|'Kbps'|'Gbps';
export type Page='overview'|'connections'|'rules'|'speed'|'settings';
export interface AppUpdate{status:string;version:string|null;notes:string|null;downloaded:number;total:number|null;error:string|null}
export interface Rule{pattern:string;route:Route}
export interface CheckTarget{name:string;host:string;port:number}
export interface Config{port:number;lan_id:string;usb_id:string;adb_path:string;rules:Rule[];check_targets:CheckTarget[]}
export interface SettingsBackup{format:'netbridge-settings';version:1;config:Config;preferences:{unit:Unit}}
export interface ImportPreview{backup:SettingsBackup;revision:string;warnings:string[]}
export interface Adapter{id:string;name:string;description:string;ip:string;dns:string[];index:number;if_type:number}
export interface Device{serial:string;model:string;state:string;usb:boolean}
export interface Probe{ok:boolean;sourceIp:string|null;predictedSourceIp:string|null;adapter:string|null;remoteIp:string|null;detail:string}
export interface Target{name:string;host:string;port:number;route:Route;ok:boolean;detail:string;checkedAt:string;windows?:Probe;selected?:Probe}
export interface Speed{adapter:string;sourceIp:string;downloadMbps:number|null;uploadMbps:number|null;latencyMs:number|null;grade:string;explanation:string;errors:string[]}
export interface Snapshot{
 mode:'live'|'preview'|'demo';version:string;config:Config;configRevision:string;configChanged:boolean;exitAllowed?:boolean;adapters:Adapter[];
 configError?:string|null;recoveryAvailable?:boolean;
 session:{kind:string;owned:boolean;label:string;detail:string;endpoint:string};
 job:{id:number;kind:string;phase:string;active:boolean;cancelling:boolean;error:string|null};
 targets:Target[];speeds:Speed[];logs:string[];unit:Unit;adbLatest?:{version:string}|null;
}
export const demoTargets=[
 {name:'Portal',host:'portal.example.test',port:443,route:'lan' as Route,initial:'G'},
 {name:'Service',host:'192.0.2.20',port:12443,route:'lan' as Route,initial:'S'},
 {name:'Application',host:'www.example.org',port:443,route:'usb' as Route,initial:'O'},
 {name:'Broker',host:'198.51.100.20',port:1883,route:'usb' as Route,initial:'M'}
];
export function convert(value:number|null,unit:Unit):number|null{
 if(value===null||!Number.isFinite(value)||value<0)return null;
 return value*({Mbps:1,'MB/s':1/8,'MiB/s':1e6/8/1048576,Kbps:1000,Gbps:0.001}[unit]);
}

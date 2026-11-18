import {Adapter,SettingsBackup} from './model';
import {SelectOption} from './select';

export function restoreAdapterOptions(adapters:Adapter[],saved:string,selected:string):SelectOption[]{
 const options:SelectOption[]=[{value:'',label:'Chưa chọn adapter'},...adapters.map(a=>({value:a.id,label:a.name+' · '+a.ip}))];
 if(saved&&!options.some(option=>option.value===saved))options.push({value:saved,label:'Giữ adapter đã lưu'});
 if(selected&&!options.some(option=>option.value===selected))options.push({value:selected,label:'Giữ adapter vừa chọn'});
 return options;
}
export function remapBackup(source:SettingsBackup,lan:string,usb:string,bundledAdb:boolean):SettingsBackup{
 const backup=structuredClone(source);backup.config.lan_id=lan;backup.config.usb_id=usb;
 if(bundledAdb)backup.config.adb_path='auto';
 return backup;
}
export function restoreWarnings(backup:SettingsBackup,adapters:Adapter[]):string[]{
 const warnings:string[]=[];
 for(const [role,id] of [['LAN',backup.config.lan_id],['USB',backup.config.usb_id]]){
  if(id&&!adapters.some(adapter=>adapter.id===id))warnings.push('Adapter '+role+' đã chọn chưa hoạt động trên máy này. Có thể chọn adapter khác hoặc giữ để kết nối sau.');
  else if(!id&&backup.config.rules.some(rule=>rule.route===role.toLowerCase()))warnings.push('Chưa chọn adapter '+role+' cho các quy tắc trong file.');
 }
 if(backup.config.lan_id&&backup.config.lan_id===backup.config.usb_id)warnings.push('LAN và USB đang trỏ cùng một adapter; hai vai trò sẽ không tạo hai đường mạng khác nhau.');
 if(backup.config.adb_path!=='auto')warnings.push('Đường dẫn ADB tùy chỉnh cần tồn tại và đáng tin cậy trên máy này. Có thể chọn ADB tích hợp để dùng trên máy này.');
 return warnings;
}

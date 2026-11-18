import {Component,ElementRef,HostListener,Input,ViewChild,forwardRef,OnChanges,OnDestroy,ChangeDetectorRef} from '@angular/core';
import {ControlValueAccessor,NG_VALUE_ACCESSOR} from '@angular/forms';
export interface SelectOption{value:string;label:string}
@Component({
 selector:'nb-select',standalone:true,
 providers:[{provide:NG_VALUE_ACCESSOR,useExisting:forwardRef(()=>Select),multi:true}],
 template:`<button #trigger type="button" class="select-trigger" role="combobox" [id]="inputId"
  [disabled]="disabled" [attr.aria-label]="label" aria-haspopup="listbox" [attr.aria-expanded]="opened"
  [attr.aria-controls]="listId" [attr.aria-activedescendant]="opened ? listId+'-'+active : null"
  (click)="toggle()" (keydown)="onKey($event)" (blur)="touch()">
  <span class="select-value">{{selectedLabel()}}</span><svg class="select-chevron" viewBox="0 0 20 20" aria-hidden="true"><path d="m6 8 4 4 4-4"/></svg>
 </button>
 <div #menu class="select-menu" popover="manual" role="listbox" [id]="listId" [attr.aria-label]="label"
  [style.left.px]="left" [style.top.px]="top" [style.width.px]="width" [style.max-height.px]="maxHeight"
  (pointerdown)="$event.preventDefault()">
  @for(option of options;track option.value;let i=$index){
   <div class="select-option" role="option" [id]="listId+'-'+i" [attr.aria-selected]="option.value===value"
    [class.highlighted]="i===active" (pointermove)="active=i" (click)="choose(i)">
    <span>{{option.label}}</span><svg class="select-check" [class.visible]="option.value===value" viewBox="0 0 20 20" aria-hidden="true"><path d="m5 10 3 3 7-7"/></svg>
   </div>
  }@empty{<div class="select-empty">Chưa có lựa chọn</div>}
 </div>`
})
export class Select implements ControlValueAccessor,OnChanges,OnDestroy{
 private static serial=0;
 @Input() inputId='';@Input() label='';@Input() options:SelectOption[]=[];@Input() disabled=false;
 @ViewChild('trigger',{static:true}) trigger!:ElementRef<HTMLButtonElement>;
 @ViewChild('menu',{static:true}) menu!:ElementRef<HTMLElement>;
 readonly listId='nb-select-'+(++Select.serial);
 value='';opened=false;active=0;left=0;top=0;width=0;maxHeight=280;
 private search='';private searchAt=0;
 private change:(value:string)=>void=()=>{};touch:()=>void=()=>{};
 private readonly dismissOnScroll=(event:Event)=>{if(this.opened&&!this.menu.nativeElement.contains(event.target as Node))this.close();};
 constructor(private readonly element:ElementRef<HTMLElement>,private readonly detector:ChangeDetectorRef){document.addEventListener('scroll',this.dismissOnScroll,true);}
 writeValue(value:string){this.value=value??'';}
 registerOnChange(fn:(value:string)=>void){this.change=fn;}
 registerOnTouched(fn:()=>void){this.touch=fn;}
 setDisabledState(disabled:boolean){this.disabled=disabled;if(disabled)this.close();}
 ngOnChanges(){if(this.disabled||!this.options.length)this.close();this.active=Math.min(this.active,Math.max(0,this.options.length-1));}
 ngOnDestroy(){document.removeEventListener('scroll',this.dismissOnScroll,true);}
 selectedLabel(){return this.options.find(option=>option.value===this.value)?.label||'Chọn một mục';}
 toggle(){if(this.opened)this.close();else this.open();}
 open(){
  if(this.disabled||!this.options.length)return;
  this.active=Math.max(0,this.options.findIndex(option=>option.value===this.value));
  const rect=this.trigger.nativeElement.getBoundingClientRect();
  this.width=Math.min(Math.max(rect.width,180),window.innerWidth-16);
  this.left=Math.min(Math.max(8,rect.left),window.innerWidth-this.width-8);
  const below=window.innerHeight-rect.bottom-14,above=rect.top-14;
  const upwards=below<Math.min(240,this.options.length*40+12)&&above>below;
  this.maxHeight=Math.max(40,Math.min(280,upwards?above:below));
  const height=Math.min(this.maxHeight,this.options.length*40+12);
  this.top=upwards?Math.max(8,rect.top-height-6):rect.bottom+6;
  this.opened=true;this.menu.nativeElement.showPopover();
  requestAnimationFrame(()=>this.scrollActive());
 }
 close(){if(!this.opened)return;this.opened=false;this.menu.nativeElement.hidePopover();this.search='';this.detector.markForCheck();}
 choose(index:number){
  const option=this.options[index];if(this.disabled||!option)return;
  if(this.value!==option.value){this.value=option.value;this.change(this.value);}
  this.touch();this.close();this.trigger.nativeElement.focus();
 }
 private scrollActive(){this.menu.nativeElement.children[this.active]?.scrollIntoView({block:'nearest'});}
 onKey(event:KeyboardEvent){
  if(this.disabled||!this.options.length)return;
  if(event.key==='Tab'){this.close();return;}
  if(event.key==='Escape'&&this.opened){event.preventDefault();event.stopPropagation();this.close();return;}
  if(['Enter',' ','ArrowDown','ArrowUp','Home','End'].includes(event.key)){
   event.preventDefault();event.stopPropagation();
   if(!this.opened){this.open();if(event.key==='End')this.active=this.options.length-1;}
   else if(event.key==='Enter'||event.key===' '){this.choose(this.active);return;}
   else if(event.key==='Home')this.active=0;
   else if(event.key==='End')this.active=this.options.length-1;
   else this.active=(this.active+(event.key==='ArrowDown'?1:-1)+this.options.length)%this.options.length;
   this.scrollActive();return;
  }
  if(event.key.length===1&&!event.ctrlKey&&!event.altKey&&!event.metaKey){
   event.preventDefault();if(!this.opened)this.open();
   const now=Date.now();this.search=now-this.searchAt>700?event.key:this.search+event.key;this.searchAt=now;
   const index=this.options.findIndex(option=>option.label.toLocaleLowerCase().startsWith(this.search.toLocaleLowerCase()));
   if(index>=0){this.active=index;this.scrollActive();}
  }
 }
 @HostListener('document:pointerdown',['$event']) outside(event:PointerEvent){if(!this.element.nativeElement.contains(event.target as Node))this.close();}
 @HostListener('window:resize') resize(){this.close();}
}

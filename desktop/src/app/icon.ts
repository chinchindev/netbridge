import {Component,input} from '@angular/core';
@Component({selector:'nb-icon',standalone:true,template:'<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">@for (path of paths(); track $index) {<path [attr.d]="path"/>}</svg>'})
export class Icon{
 name=input('grid');
 paths(){return ({
 grid:['M3 3h7v7H3zM14 3h7v7h-7zM3 14h7v7H3zM14 14h7v7h-7z'],
 network:['M4 16h5v5H4zM15 16h5v5h-5zM9 3h6v5H9zM12 8v4M6.5 16v-4h11v4'],
 rules:['M8 6h13M8 12h13M8 18h13M3 6h.01M3 12h.01M3 18h.01'],
 gauge:['M3.5 19a10 10 0 1 1 17 0M12 13l5-5M6 19h12'],
 settings:['M9 4l1-2h4l1 2 2 1 2-.2 2 3-1 2v3l1 2-2 3-2-.2-2 1-1 2h-4l-1-2-2-1-2 .2-2-3 1-2v-3l-1-2 2-3 2 .2z','M15 11a3 3 0 1 1-6 0 3 3 0 0 1 6 0'],
 shield:['M12 3l8 3v6c0 5-8 9-8 9s-8-4-8-9V6zM8 12l3 3 5-6'],
 usb:['M12 21V3m-3 3 3-3 3 3M12 15l-5-4V8m5 10 5-4v-4M5 6h4v3H5zM15 7h4v3h-4z'],
 arrow:['M5 12h14m-5-5 5 5-5 5'],check:['M5 12l4 4L19 6'],close:['M6 6l12 12M18 6 6 18'],
 refresh:['M20 7v5h-5M4 17v-5h5','M6.5 6a7 7 0 0 1 11.8 1L20 12M4 12l1.7 5A7 7 0 0 0 17.5 18'],
 play:['M8 4l13 8-13 8z'],stop:['M6 6h12v12H6z'],plus:['M12 5v14M5 12h14'],
 down:['M12 4v16m-6-6 6 6 6-6'],up:['M12 20V4m-6 6 6-6 6 6'],
 info:['M12 10v7M12 7h.01','M22 12a10 10 0 1 1-20 0 10 10 0 0 1 20 0'],
 log:['M5 3h14v18H5zM8 7h8M8 11h8M8 15h5'],clock:['M12 7v5l3 2','M22 12a10 10 0 1 1-20 0 10 10 0 0 1 20 0'],
 monitor:['M3 4h18v13H3zM8 21h8M12 17v4'],chevron:['M9 5l7 7-7 7'],
 trash:['M3 6h18M9 6V3h6v3M5 6l1 15h12l1-15M10 10v7M14 10v7']
 } as Record<string,string[]>)[this.name()]??['M12 4v16M4 12h16'];}
}

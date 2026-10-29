/* Focused 128 px scenes: primary 30 px, secondary 20 px, quiet labels. */
static void metric_value(Row *r,unsigned x,unsigned y,unsigned value,int valid,unsigned font,uint16_t ink){
 char out[12];if(valid)number(out,value);else{out[0]='-';out[1]='-';out[2]=0;}modern_text(r,out,x,y,font,ink);
}
static void metric_temp(Row *r,unsigned x,unsigned y,const char *label,const Frame *f,unsigned index,uint16_t ink,uint16_t accent){
 modern_text(r,label,x,y,0,accent);const uint8_t *t=f->telemetry;
 int valid=f->fresh&&t[32]==1&&(t[33]&(1u<<index));int v=(int16_t)get16(t+34+index*2);
 char out[12];if(valid){char *p=out;if(v<0){*p++='-';v=-v;}number(p,divide((unsigned)v+5,10));}else{out[0]='-';out[1]='-';out[2]=0;}
 unsigned font=modern_width(out,1)<=36 ? 1 : 0;modern_text(r,out,x,y+18,font,ink);
 /* Degree mark and C use the small alphabet, outside the large numeral. */
 unsigned w=modern_width(out,font),cx=x+w+2;rect(r,cx,y+18,2,2,accent);text(r,"C",cx+3,y+22,1,accent);
}
static void metric_graph(Row *r,const Frame *f,unsigned y,unsigned shift,uint16_t ink,uint16_t background){
 pill(r,8,y+26,112,1,covered(ink,background,1));
 if(!f->fresh)return;
 for(unsigned i=0;i<f->history_count && i<32;++i){unsigned v=(f->history[i]>>shift)&15;
  if(v==15)continue;unsigned h=divide(v*24,14)+1,x=117-i*3;rect(r,x,y+25-h,2,h,ink);
 }
}
static void metrics_screen(Row *r,const Frame *f,uint16_t ink,uint16_t accent,uint16_t background){
 const uint8_t *t=f->telemetry;int g=f->fresh&&t[16]==1;char out[16];
 if(f->kind==9){
  modern_text(r,"FPS",8,6,0,accent);int valid=g&&(t[17]&4);unsigned fps=divide(get16(t+28)+5,10);
  metric_value(r,8,27,fps,valid,2,ink);
  /* Average present interval, not a fabricated latency/1% low measurement. */
  if(valid){unsigned ms=divide(100000,get16(t+28));char *p=number(out,divide(ms,10));*p++='.';*p++='0'+ms-divide(ms,10)*10;*p=0;
   modern_text(r,out,8,64,0,accent);modern_text(r,"MS",8+modern_width(out,0)+5,64,0,accent);}
  metric_temp(r,8,85,"CPU",f,0,ink,accent);metric_temp(r,70,85,"GPU",f,1,ink,accent);
 }else if(f->kind==10){
  metric_temp(r,8,6,"CPU",f,0,ink,accent);metric_temp(r,70,6,"GPU",f,1,ink,accent);
  metric_temp(r,8,68,"DISK",f,2,ink,accent);metric_temp(r,70,68,"SYS",f,3,ink,accent);
 }else if(f->kind==11){
  modern_text(r,"GPU",8,6,0,accent);modern_text(r,"%",105,41,0,accent);
  metric_value(r,8,27,divide(get16(t+18),10),g&&(t[17]&1),2,ink);
  if(f->fresh&&t[32]==1&&(t[33]&16)){number(out,divide(get16(t+42)+5,10));unsigned w=modern_width(out,0);modern_text(r,out,110-w,6,0,ink);text(r,"W",114,10,1,accent);}
  text(r,"VRAM %",8,88,1,accent);unsigned used=get32(t+20),total=get32(t+24),q=0;
  if(g&&(t[17]&2))q=divide(used*100,total);
  metric_value(r,8,103,q,g&&(t[17]&2),1,ink);metric_temp(r,70,85,"GPU",f,1,ink,accent);
 }else{
  modern_text(r,"CPU %",8,5,0,accent);metric_value(r,72,2,divide(get16(t),10),f->fresh,1,ink);
  metric_graph(r,f,29,0,accent,background);
  modern_text(r,"GPU %",8,68,0,accent);metric_value(r,72,65,divide(get16(t+18),10),g&&(t[17]&1),1,ink);
  metric_graph(r,f,92,4,ink,background);
 }
}

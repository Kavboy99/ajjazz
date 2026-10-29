/* Four primary readings, one quiet footer. */
static void game_screen(Row *row,const Frame *f,uint16_t ink,uint16_t accent){
 const uint8_t *c=f->config,*t=f->telemetry;int extended=f->fresh&&t[16]==1;
 modern_percent(row,8,5,"CPU",get16(t),f->fresh,c,ink,accent);
 modern_percent(row,70,5,"RAM",get16(t+2),f->fresh,c,ink,accent);
 modern_percent(row,8,57,"GPU",get16(t+18),extended&&(t[17]&1),c,ink,accent);
 /* Split the product to retain precision at the protocol's 8 TiB limit. */
 unsigned used=get32(t+20),total=get32(t+24),q=0,vram=0;
 if(extended&&(t[17]&2)){q=divide(used*125,total);vram=q*8+divide((used*125-q*total)*8,total);}
 modern_percent(row,70,57,"VRAM",vram,extended&&(t[17]&2),c,ink,accent);
 char fps[8];if(extended&&(t[17]&4))number(fps,divide(get16(t+28)+5,10));
 else{fps[0]='-';fps[1]='-';fps[2]=0;}
 modern_text(row,"FPS",8,112,0,accent);
 if(c[0]&16){
  modern_text(row,fps,39,112,0,ink);
  if(extended&&(t[17]&2)){
   unsigned tenth=divide(get32(t+20)*10,1024);char out[12],*p=number(out,divide(tenth,10));
   if(tenth<100){*p++='.';*p++='0'+tenth-divide(tenth,10)*10;}*p++='G';*p=0;
   unsigned w=modern_width(out,0);if(w+39+modern_width(fps,0)+8<=120)modern_text(row,out,120-w,112,0,ink);
  }
 }else{unsigned w=modern_width(fps,1);modern_text(row,fps,w<120?120-w:0,104,1,ink);}
}

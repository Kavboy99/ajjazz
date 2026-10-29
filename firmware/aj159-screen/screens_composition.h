/* UI3: page[1] is duration, page[2..3] hold four metric nibbles. */
static int valid_composition(unsigned packed){
 unsigned used=0,empty=0;
 for(unsigned i=0;i<4;++i){unsigned m=(packed>>(i*4))&15;if(!m){empty=1;continue;}if(empty||m>11||(used&(1u<<m)))return 0;used|=1u<<m;}
 return used!=0;
}
static const char *composition_label(unsigned m){
 const char *names[]={"","CPU %","RAM %","GPU %","VRAM%","FPS","CPU","GPU","DISK","SYS","GPU W","TIME"};return names[m];
}
static int composition_number(char *out,unsigned m,const Frame *f){
 const uint8_t *t=f->telemetry;unsigned value=0;
 if(m==11){clock_label(out,f);return 1;}if(!f->fresh)return 0;
 if(m<=2)value=divide(get16(t+(m-1)*2),10);
 else if(m==3){if(!(t[17]&1))return 0;value=divide(get16(t+18),10);}
 else if(m==4){unsigned total=get32(t+24),used=get32(t+20);if(!(t[17]&2)||!total)return 0;if(used>total)used=total;value=divide(used*100,total);}
 else if(m==5){if(!(t[17]&4))return 0;value=divide(get16(t+28)+5,10);}
 else if(m<=9){unsigned index=m-6;if(t[32]!=1||!(t[33]&(1u<<index)))return 0;int v=(int16_t)get16(t+34+index*2);if(v<0){*out++='-';v=-v;}value=divide((unsigned)v+5,10);}
 else {if(t[32]!=1||!(t[33]&16))return 0;value=divide(get16(t+42)+5,10);}
 number(out,value);return 1;
}
static void composition_backdrop(Row *r,const Frame *f,uint16_t accent,uint16_t bg){
 unsigned effect=f->kind-13,ph=f->asset_frame,y=r->y;if(effect>=6)effect-=6;if(!effect)return;
 uint16_t ink=covered(accent,bg,1);
 for(unsigned x=0;x<128;++x){if(x>=4&&x<124&&y>=4&&y<118)continue;unsigned on=0;
  if(effect==1){unsigned t=ph&31;if(t>15)t=31-t;on=y==124&&x>=64-t*3&&x<=64+t*3;}
  else if(effect==2||effect==4){unsigned pos=y==2?x:x==125?125+y:y==125?375-x:x==2?500-y:1000;
   if(effect==2){unsigned a=ph*3-divide(ph*3,500)*500,b=pos+500-a;on=pos<500&&b-divide(b,500)*500<8;}
   else {unsigned a=ph*2-divide(ph*2,125)*125,b=pos+500-a;on=pos<500&&b-divide(b,125)*125<2;}}
  else if(effect==3){unsigned a=(x+ph)&15;if(a>7)a=15-a;on=y==122-divide(a,2);}
  else if(effect==5){unsigned a=(x+128-((ph*3)&127))&127;on=(y==2||y==125)&&a<20;}
  if(on)rect(r,x,y,1,1,ink);
 }
}
static void composition_screen(Row *r,const Frame *f,uint16_t ink,uint16_t accent,uint16_t bg){
 composition_backdrop(r,f,accent,bg);unsigned packed=get16(f->options),count=0;
 while(count<4&&((packed>>(count*4))&15))++count;
 for(unsigned i=0;i<count;++i){unsigned m=(packed>>(i*4))&15,x=10,y=18,w=108,h=92;
  if(count==2){y=i?70:8;h=52;}
  else if(count==3){y=i?76:7;h=i?46:55;if(i){w=50;x=i==1?10:70;}}
  else if(count==4){x=(i&1)?70:10;y=i<2?8:72;w=50;h=50;}
  unsigned label_width=modern_width(composition_label(m),0)+(m>=6&&m<=9?15:0),lx=x+(label_width<w?divide(w-label_width,2):0);
  modern_text(r,composition_label(m),lx,y,0,accent);
  if(m>=6&&m<=9){unsigned cx=lx+modern_width(composition_label(m),0)+2;rect(r,cx,y+1,3,1,accent);rect(r,cx,y+2,1,1,accent);rect(r,cx+2,y+2,1,1,accent);rect(r,cx,y+3,3,1,accent);modern_text(r,"C",cx+5,y,0,accent);}
  char out[12];int valid=composition_number(out,m,f);
  if(!valid){out[0]='-';out[1]='-';out[2]=0;}unsigned font=w>60?2:4;if(font==4&&modern_width(out,font)>w)font=0;else while(font&&modern_width(out,font)>w)--font;
  modern_text(r,out,x+divide(w-modern_width(out,font),2),y+(w>60?19:18),font,ink);
  if(f->kind>=19&&m<=4){unsigned value=0;if(valid){const char *p=out;while(*p>='0'&&*p<='9')value=value*10+*p++-'0';if(value>100)value=100;}
   pill(r,x,y+h-4,w,3,covered(accent,bg,1));if(valid&&value)pill(r,x,y+h-4,divide(w*value,100),3,accent);}
 }
}

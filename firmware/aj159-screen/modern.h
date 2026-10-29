/* 2-bit antialiased glyphs from Inter, SIL OFL1.1. See docs/licenses/Inter-OFL.txt. */
#include "modern-font.h"
static int glyph_index(unsigned font,unsigned ch){
 if(ch>='0'&&ch<='9')return ch-'0';
 if(!font){if(ch>='A'&&ch<='Z')return ch-'A'+10;
  const char *p=":./%+- ";for(unsigned i=0;p[i];++i)if(ch==(unsigned)p[i])return 36+i;
 }else if(font<3||font==4){if(ch==':')return 10;if(ch=='.')return 11;if(ch=='-')return 12;}
 return -1;
}
static unsigned modern_width(const char *s,unsigned font){
 unsigned width=0;while(*s){int i=glyph_index(font,(uint8_t)*s++);if(i>=0)width+=modern_fonts[font].glyphs[i].advance;}return width;
}
static uint16_t covered(uint16_t foreground,uint16_t background,unsigned a){
 if(a==3)return foreground;
 unsigned r=((((foreground>>11)*a+(background>>11)*(3-a)+1)*171)>>9);
 unsigned g=(((((foreground>>5)&63)*a+((background>>5)&63)*(3-a)+1)*171)>>9);
 unsigned b=((((foreground&31)*a+(background&31)*(3-a)+1)*171)>>9);
 return (r<<11)|(g<<5)|b;
}
static void modern_text(Row *row,const char *s,unsigned x,unsigned y,unsigned font,uint16_t ink){
 const ModernFont *f=modern_fonts+font;
 if(row->y<y||row->y>=y+f->height)return;
 unsigned dy=row->y-y;
 while(*s){int i=glyph_index(font,(uint8_t)*s++);if(i<0)continue;
  const ModernGlyph *g=f->glyphs+i;
  for(unsigned col=0;col<g->width&&x+col<128;++col){unsigned bit=dy*g->width+col;
   unsigned a=(f->data[g->offset+(bit>>2)]>>(6-(bit&3)*2))&3;
   if(a)row->pixels[x+col]=covered(ink,row->pixels[x+col],a);
  }x+=g->advance;
 }
}
static void modern_center(Row *r,const char *s,unsigned y,unsigned font,uint16_t ink){
 unsigned w=modern_width(s,font);modern_text(r,s,w<128?divide(128-w,2):0,y,font,ink);
}
static void pill(Row *r,unsigned x,unsigned y,unsigned w,unsigned h,uint16_t ink){
 if(r->y<y||r->y>=y+h)return;
 unsigned dy=r->y-y,inset=(dy==0||dy+1==h)?2:(dy==1||dy+2==h)?1:0;
 if(w>inset*2)rect(r,x+inset,r->y,w-inset*2,1,ink);
}
static void modern_bar(Row *r,unsigned x,unsigned y,unsigned w,unsigned h,unsigned value,int fresh,uint16_t ink,uint16_t background){
 pill(r,x,y,w,h,covered(ink,background,1));
 if(fresh){unsigned fill=divide(w*value,1000);if(fill)pill(r,x,y,fill,h,ink);}
}
static void modern_percent(Row *r,unsigned x,unsigned y,const char *label,unsigned value,int fresh,const uint8_t *c,uint16_t ink,uint16_t accent){
 modern_text(r,label,x,y,0,accent);if(c[0]&4)modern_text(r,"%",x+44,y,0,accent);
 char out[8];if(fresh)number(out,divide(value,10));else{out[0]='-';out[1]='-';out[2]=0;}
 if(c[0]&4)modern_text(r,out,x,y+18,1,ink);
 if(c[0]&8)modern_bar(r,x,y+((c[0]&4)?41:21),50,(c[0]&4)?4:14,value,fresh,accent,color(c+15));
}
static void modern_detail(Row *r,const Frame *f,uint16_t ink,uint16_t accent){
 const uint8_t *c=f->config;char out[32];
 if(f->kind==3){
  if(f->options[3]){
   unsigned year=get16(f->year);char *p=out;
   p=number(p,f->day);*p++='.';p=number(p,f->month);*p++='.';number(p,year);
   modern_center(r,out,9,0,accent);
  }
  clock_label(out,f);modern_center(r,out,43,f->options[0]>1?2:1,ink);
  if(f->options[4]){out[0]=':';out[1]='0'+divide(f->second,10);out[2]='0'+f->second-divide(f->second,10)*10;out[3]=0;modern_center(r,out,99,0,accent);}
 }else{
  unsigned index=f->kind-1,value=get16(f->telemetry+index*2);
  modern_center(r,index?"RAM":"CPU",9,0,accent);
  if(f->fresh)number(out,divide(value,10));else{out[0]='-';out[1]='-';out[2]=0;}
  if(c[0]&4){unsigned font=f->options[5]>1?2:1;modern_center(r,out,36,font,ink);modern_text(r,"%",109,62,0,accent);}
  if(c[0]&8)modern_bar(r,8,89,112,7,value,f->fresh,accent,color(c+15));
  if(index&&(c[0]&16)){
   memory_label(out,f->telemetry,f->fresh);
   if(modern_width(out,0)>112){char *p=number(out,divide(get32(f->telemetry+4),1024));*p++='G';*p=0;}
   modern_center(r,out,111,0,accent);
  }else if(c[0]&2){clock_label(out,f);modern_center(r,out,111,0,accent);}
 }
}

static void modern_summary(Row *r,const Frame *f,uint16_t ink,uint16_t accent){
 const uint8_t *c=f->config;char out[32];
 if(c[44]==1){
  if(c[0]&2){clock_label(out,f);modern_center(r,out,5,2,ink);}
  modern_percent(r,8,48,"CPU",get16(f->telemetry),f->fresh,c,ink,accent);
  modern_percent(r,70,48,"RAM",get16(f->telemetry+2),f->fresh,c,ink,accent);
  if(c[0]&16){memory_label(out,f->telemetry,f->fresh);if(modern_width(out,0)<=112)modern_center(r,out,111,0,accent);}
 }else{
  for(unsigned i=0;i<2;++i){unsigned y=i?67:5;
   modern_text(r,i?"RAM":"CPU",8,y,0,accent);
   unsigned value=get16(f->telemetry+i*2);
   if(f->fresh)number(out,divide(value,10));else{out[0]='-';out[1]='-';out[2]=0;}
   if(c[0]&4){modern_text(r,"%",51,y,0,accent);modern_text(r,out,8,y+18,i?1:2,ink);}
   if(c[0]&8)modern_bar(r,8,y+((c[0]&4)?(i?39:48):22),112,(c[0]&4)?3:12,value,f->fresh,accent,color(c+15));
  }
  if(c[0]&2){clock_label(out,f);unsigned w=modern_width(out,0);modern_text(r,out,120-w,111,0,accent);}
 }
}

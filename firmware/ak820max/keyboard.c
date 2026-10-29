/* AJKS v1, ONLY support image SHA256 58db8a73...a1b116ff, SN34F280.
 * Cooperative procedural lighting and encoder actions. No host frame stream.
 * Persistent writes are bounded to two alternating 512-byte banks per record.
 * All native key/Hall/radio/LED transport code remains in place.
 */
#include <stdint.h>
#include <stddef.h>
#include "positions.h"
#include "build_id.h"
#define V8(a) (*(volatile uint8_t *)(a))
#define MILLIS (*(volatile uint32_t *)0x20000238u)
#define NATIVE(a) ((void (*)(void))((a)|1u))
#define INPUT ((volatile uint8_t *)0x20000080)
#define LIGHT ((volatile uint8_t *)0x2000019c)
#define SCENES 0x24000u
#define CONFIG 0x26000u
#define STATE ((State *)0x20007000)
#define MAGIC 0x534b4a43u /* New RAM layout; never reuse a previous extension's warm state. */
typedef struct {
 uint32_t magic, frame;
 uint16_t pressed_at[128];
 uint8_t pressed[16], scene[512], config[512], staging[512];
 uint16_t chunks;
 uint8_t staging_kind, staging_slot, active, native_mode, paused;
 uint8_t last_key, has_key, last_gpio, stable_gpio, debounce;
 volatile int8_t turns;
 uint8_t press_owned;
 uint16_t consumer; /* Only release the consumer event we own. */
 uint8_t consumer_ticks;
 uint32_t last_ms;
 uint16_t remainder_ms;
 uint8_t dirty;
 uint32_t hold_started, battery_released;
 uint8_t hold_down, hold_fired, battery_shown, overlay_saved;
 uint8_t saved_pwm[378];
} State;
_Static_assert(sizeof(State) <= 0x1000, "Reserved SRAM overflow");

static void copy(uint8_t *d,const uint8_t *s,unsigned n){while(n--)*d++=*s++;}
static void zero(uint8_t *d,unsigned n){while(n--)*d++=0;}
static uint16_t rd16(const uint8_t *p){return p[0]|((uint16_t)p[1]<<8);}
static void wr16(uint8_t *p,uint16_t n){p[0]=n;p[1]=n>>8;}
static uint16_t crc(const uint8_t *p,unsigned n){
 uint16_t v=0xffff;while(n--){v^=(uint16_t)*p++<<8;for(unsigned j=0;j<8;j++)v=(v<<1)^((v&0x8000)?0x1021:0);}return v;
}
static int is_page(const uint8_t *p,int config){
 if(p[0]!='A'||p[1]!='K'||p[2]!=(config?'C':'L')||p[3]!=(config?'F':'P')||p[4]!=1||rd16(p+510)!=crc(p,510))return 0;
 if(config){
  if(p[5]!=255&&p[5]>=8)return 0;
  if(p[7]<8||p[7]>248||p[10]>1)return 0;
  if(p[28] && (p[28]!=1||p[29]>1||p[30]>1||p[31]>1||p[32]<1||p[32]>5||p[33]<1||p[33]>10))return 0;
  for(unsigned i=0;i<3;i++){
   const uint8_t *a=p+16+4*i;unsigned v=rd16(a+2);
   if(a[0]>6||a[1])return 0;
   if(a[0]==1 && (v==0||v>0x3ff))return 0;
   if((a[0]==2||a[0]==3||a[0]==5) && (v!=1&&v!=65535))return 0;
  }
 }else{
  if(p[5]>8)return 0;
  for(unsigned i=0;i<p[5];i++){
   const uint8_t *l=p+64+i*48;
   if(l[0]>1||l[1]>21||l[2]>2||rd16(l+10)>2048||rd16(l+12)<16||rd16(l+12)>2048)return 0;
   int x=(int16_t)rd16(l+14),y=(int16_t)rd16(l+16);
   if(x < -256||x>256||y < -256||y>256)return 0;
  }
 }
 return 1;
}
static const uint8_t *newest(unsigned address,int config){
 const uint8_t *a=(const uint8_t *)address,*b=a+512;
 int av=is_page(a,config),bv=is_page(b,config);
 if(!av)return bv?b:0;if(!bv)return a;
 return (int16_t)(rd16(b+8)-rd16(a+8))>0?b:a;
}
static int store(unsigned address,uint8_t *p,int config){
 const uint8_t *old=newest(address,config);
 unsigned dest=old==(const uint8_t *)address?address+512:address;
 wr16(p+8,old?(uint16_t)(rd16(old+8)+1):1);wr16(p+510,crc(p,510));
 /* Original single-page RMW writer; never pass a crossing/unaligned region. */
 ((void (*)(const void *,unsigned,unsigned))0x5cb9)(p,512,dest);
 const uint8_t *saved=(const uint8_t *)dest;
 if(!is_page(saved,config))return 0;
 for(unsigned i=0;i<512;i++)if(p[i]!=saved[i])return 0;
 return 1;
}
static void load_scene(void){
 State *s=STATE;const uint8_t *p=0;
 if(s->config[5]<8)p=newest(SCENES+1024*s->config[5],0);
 s->active=p?s->config[5]:255;
 if(p)copy(s->scene,p,512);
 if(p){LIGHT[4]=20;LIGHT[0x15]=255;LIGHT[0x0e]=0;}
 s->native_mode=LIGHT[4];s->paused=s->config[10];
 s->dirty=1;
}
static void init(void){
 State *s=STATE;if(s->magic==MAGIC)return;
 zero((uint8_t *)s,sizeof(*s));
 const uint8_t *p=newest(CONFIG,1);
 if(p)copy(s->config,p,512);
 else{
  copy(s->config,(const uint8_t *)"AKCF",4);s->config[4]=1;s->config[5]=255;s->config[6]=255;s->config[7]=128;
  /* Default actions: native volume down/up and native press/mute. */
 }
 s->last_gpio=s->stable_gpio=((unsigned (*)(void))0x1215d)();
 load_scene();s->last_ms=MILLIS;s->magic=MAGIC;
}
/* OEM SysTick ataff8 ->46f8 maintains the HAL millisecond counter. The
 * cooperative main-loop hook is NOT a50Hz clock and must not count frames.
 */
static unsigned advance_clock(void){
 State *s=STATE;uint32_t now=MILLIS,elapsed=now-s->last_ms;s->last_ms=now;
 if(s->paused||s->active==255){s->remainder_ms=0;return 0;}
 uint32_t frames=elapsed/20,rest=elapsed%20+s->remainder_ms;
 frames+=rest/20;s->remainder_ms=rest%20;s->frame+=frames;return frames;
}
static uint8_t wave(unsigned x){x&=255;return x<128?x*2:510-x*2;}
static uint32_t noise(uint32_t n){n^=n>>16;n*=0x7feb352d;n^=n>>15;n*=0x846ca68b;return n^(n>>16);}
static unsigned absi(int x){return x<0?(unsigned)-x:(unsigned)x;}
static unsigned min(unsigned a,unsigned b){return a<b?a:b;}
static uint8_t weight(const uint8_t *l,unsigned id){
 State *s=STATE;
 int x=pos[id][0],y=pos[id][1];
 int projection=(x*(int16_t)rd16(l+14)+y*(int16_t)rd16(l+16))/256;
 unsigned scale=rd16(l+12),speed=rd16(l+10);
 unsigned t=((s->frame*speed)/50u)*s->config[7]/128u+rd16(l+18);
 unsigned p=(projection*(int)scale)/256+t;
 uint32_t h=noise(id*1337u+(t>>3));
 switch(l[1]){
 case 0:return 0;
 case 1:return (uint8_t)(projection*scale/256);
 case 2:return wave(p);
 case 3:return wave(t);
 case 4:{
  if(!s->has_key)return 0;
  unsigned d=absi(x-pos[s->last_key][0])+absi(y-pos[s->last_key][1]);
  unsigned age=(uint16_t)(s->frame-s->pressed_at[s->last_key]);
  if(age>100)return 0;
  unsigned gap=absi((int)(d*scale/256)-(int)(age*6));
  return gap<36?(36-gap)*7:0;
 }
 case 5:return (h&255)>232?255:0;
 case 6:{unsigned age=(uint16_t)(s->frame-s->pressed_at[id]);return (s->pressed[id>>3]&(1u<<(id&7)))&&age<50?(50-age)*5:0;}
 case 7:return (wave(p+wave(y*scale/256+t/2)/2)+wave(p/2+t/3))/2;
 case 8:return (wave(x*scale/256+t)+wave(y*scale/128+t*2)+wave((x+y)*scale/256-t))/3;
 case 9:{unsigned d=(uint8_t)(p);return d<96?255-d*255/96:0;}
 case 10:{unsigned fall=(uint8_t)(y*scale/128+t+(noise(x/18)*3));return fall<72?255-fall*255/72:0;}
 case 11:return min(255,40+(h&127)+wave(t+x+y)/3);
 case 12:{unsigned d=absi(x-128)+absi(y-128);return wave(d*scale/128-t);}
 case 13:{unsigned d=absi(x-(int)wave(t));return d<48?(48-d)*255/48:0;}
 case 14:return (wave(p)+wave((unsigned)(projection*scale/256)-t))/2;
 case 15:return (((unsigned)x*scale/16384+(unsigned)y*scale/16384+(t>>5))&1)?255:0;
 case 16:{unsigned v=wave(t+(noise(id*7919)&255));return v*v/255*v/255;}
 case 17:{unsigned age=t&255,seed=noise((t>>8)*991);int cx=seed&255,cy=(seed>>8)&255;unsigned d=absi(x-cx)+absi(y-cy);unsigned gap=absi((int)d-(int)age*2);return gap<32?(32-gap)*(255-age)/32:0;}
 case 18:{unsigned tick=t&255;if(tick>12&&(tick<36||tick>42))return 0;unsigned column=(noise((t>>8)*31)&255);int bolt=(int)column+(int)(noise((unsigned)y/40+(t>>8))%65)-32;return absi(x-bolt)<26?255:8;}
 case 19:{unsigned tick=t&255;unsigned a=absi((int)tick-32),b=absi((int)tick-80);return a<20?(20-a)*255/20:b<14?(14-b)*180/14:0;}
 case 20:{unsigned a=wave(p+wave((unsigned)y*scale/256-t/3)),b=wave((unsigned)(x+y)*scale/384+t/2);return a*b/255;}
 case 21:{unsigned cell=(unsigned)x*scale/16384+16*((unsigned)y*scale/16384);unsigned a=noise(cell+(t>>7)*977)&255,b=noise(cell+((t>>7)+1)*977)&255,w=t&127;return (a*(128-w)+b*w)/128;}
 }
 return 0;
}
static void render(void){
 State *s=STATE;
 /* Scene/keypress IDs are the sparse Hall matrix. RGB/PWM planes use the
  * compact serpentine LED order. The OEM static renderer at7a3c performs
  * exactly this translation through ROM16338; never address planes by key ID.
  */
 for(unsigned led=0;led<126;led++){
  for(unsigned j=0;j<3;j++)V8(0x2000478c+j*126+led)=0;
  V8(0x20004906+led)=0;
 }
 for(unsigned id=0;id<126;id++){
  if(!pos[id][2])continue;
  unsigned c[3]={0,0,0};
  for(unsigned i=0;i<s->scene[5];i++){
   const uint8_t *l=s->scene+64+i*48;
   if(!l[0]||!(l[20+(id>>3)]&(1u<<(id&7))))continue;
   unsigned w=weight(l,id),alpha=l[3];
   for(unsigned j=0;j<3;j++){
    unsigned v=(l[4+j]*(255-w)+l[7+j]*w)/255;
    if((l[1]>=4&&l[1]!=7&&l[1]!=8&&l[1]!=11&&l[1]!=12&&l[1]!=14&&l[1]!=15&&l[1]!=20&&l[1]!=21))v=v*w/255;
    if(l[2]==0)c[j]=(c[j]*(255-alpha)+v*alpha)/255;
    else if(l[2]==1)c[j]=min(255,c[j]+v*alpha/255);
    else c[j]=c[j]*((255-alpha)+v*alpha/255)/255;
   }
  }
  unsigned led=key_to_led[id];
  for(unsigned j=0;j<3;j++){
   unsigned srgb=c[j]*s->scene[7]/255*s->config[6]/255;
   V8(0x2000478c+j*126+led)=led_input[srgb];
  }
  V8(0x20004906+led)=255;
 }
 NATIVE(0xd264)();NATIVE(0xcf90)();NATIVE(0xaf8c)();
 /* OEM cf90 can skip RGB entirely (2000022b) or apply a zero native gain
  * after boot/wake. AJKS owns its brightness; those flags are not its state.
  * Keep native housekeeping, then publish our bounded6-bit planes directly to
  * the UNCHANGED serializer, exactly as the autonomous battery overlay does.
  * Native lighting still owns the entire pipeline when active==255.
  */
 for(unsigned id=0;id<126;id++)if(pos[id][2]){
  unsigned led=key_to_led[id];
  for(unsigned j=0;j<3;j++)
   V8(0x20004984+j*126+led)=(unsigned)V8(0x2000478c+j*126+led)*65025u>>18;
 }
}
static void consumer(unsigned usage){
 State *s=STATE;
 /* Do not overwrite another physical consumer key. */
 unsigned old=INPUT[0x39]|((unsigned)INPUT[0x3a]<<8);
 if(old&&old!=s->consumer)return;
 if(V8(0x20005e91)==4)((void (*)(void *))0x11819)((void *)0x20005e88);
 INPUT[0x39]=usage;INPUT[0x3a]=usage>>8;INPUT[0x1e]=1;INPUT[0x18]=5;
 s->consumer=usage;s->consumer_ticks=4;
}
static void action(const uint8_t *a,int direction){
 State *s=STATE;int step=(int16_t)rd16(a+2);
 s->dirty=1;
 switch(a[0]){
 case 0:consumer(direction<0?0xea:direction>0?0xe9:0xe2);break;
 case 1:consumer(rd16(a+2));break;
 case 2:{int b=s->config[6]+step*16;s->config[6]=b<0?0:b>255?255:b;break;}
 case 3:{
  unsigned slot=s->active<8?s->active:step>0?7:0;
  for(unsigned i=0;i<8;i++){
   slot=(slot+8+step)%8;
   if(newest(SCENES+1024*slot,0)){s->config[5]=slot;load_scene();break;}
  }break;
 }
 case 4:s->config[6]=s->config[6]?0:255;break;
 case 5:{int v=s->config[7]+step*8;s->config[7]=v<8?8:v>248?248:v;break;}
 case 6:s->paused^=1;s->config[10]=s->paused;break;
 }
}
void aj_rotation(void){
 State *s=STATE;
 if(s->magic!=MAGIC || (s->config[16]==0&&s->config[20]==0)){NATIVE(0x12178)();return;}
 unsigned v=((unsigned (*)(void))0x1215d)();
 if(v!=s->last_gpio){s->last_gpio=v;s->debounce=2;return;}
 if(!s->debounce||--s->debounce)return;
 unsigned old=s->stable_gpio;s->stable_gpio=v;int d=0;
 if((old==1&&v==3)||(old==2&&v==0))d=1;
 else if((old==2&&v==3)||(old==1&&v==0))d=-1;
 int count=s->turns+d;if(count>=-16&&count<=16)s->turns=count;
}
void aj_press(void){
 init();State *s=STATE;
 if(s->config[28]==1&&s->config[29]){
  /* Defer the short action until release. A long hold must never mute or
   * change the scene first. INPUT[8] is valid only at these encoder hooks. */
  INPUT[0x39]=0;INPUT[0x3a]=0;INPUT[0x1e]=1;
  if(INPUT[8]){
   if(!s->hold_down){s->hold_started=MILLIS;s->hold_down=1;s->hold_fired=0;}
  }else if(s->hold_down){
   s->hold_down=0;
   if(s->hold_fired||MILLIS-s->hold_started>=(unsigned)s->config[32]*1000){
    s->hold_fired=1;s->battery_shown=1;s->battery_released=MILLIS;
   }else action(s->config+24,0);
  }
  return;
 }
 if(s->config[24]==0&&!s->press_owned){NATIVE(0x13d94)();return;}
 /* Native scan has just queued mute. Suppress it before the USB consumer send. */
 INPUT[0x39]=0;INPUT[0x3a]=0;INPUT[0x1e]=1;
 if(INPUT[8]){s->press_owned=1;action(s->config+24,0);}else s->press_owned=0;
}
void aj_observe(void){
 if(STATE->magic!=MAGIC||!INPUT[8]||INPUT[3]>=126)return;
 unsigned id=INPUT[3];STATE->pressed_at[id]=(uint16_t)STATE->frame;
 STATE->pressed[id>>3]|=1u<<(id&7);STATE->last_key=id;STATE->has_key=1;
 STATE->dirty=1;
}
__attribute__((naked)) void aj_key_hook(void){
 __asm volatile("push {r0-r3,r12,lr}\nbl aj_observe\npop {r0-r3,r12,lr}\npush {r4-r8,lr}\nldr r12, =0x13d99\nbx r12");
}
static void restore_overlay(void){
 State *s=STATE;if(!s->overlay_saved)return;
 for(unsigned j=0;j<378;j++)V8(0x20004984+j)=s->saved_pwm[j];
 s->overlay_saved=0;
}
static void battery_overlay(void){
 State *s=STATE;uint32_t now=MILLIS;
 if(s->config[28]!=1||!s->config[29]){s->battery_shown=0;s->hold_down=0;s->hold_fired=0;return;}
 if(s->hold_down&&now-s->hold_started>=(unsigned)s->config[32]*1000){s->hold_fired=1;s->battery_shown=1;}
 if(!s->battery_shown)return;
 if(!s->hold_down&&now-s->battery_released>=(unsigned)s->config[33]*1000){s->battery_shown=0;return;}
 unsigned percent=V8(0x2000012e); /* Same live battery byte as OEM indicator cf90 / radio GET_INFO. */
 if(percent>100){s->battery_shown=0;return;}
 for(unsigned j=0;j<378;j++)s->saved_pwm[j]=V8(0x20004984+j);
 s->overlay_saved=1;
 unsigned count=s->config[30]?82:14,filled=(percent*count+99)/100;
 for(unsigned i=0;i<count;i++){
  if(i>=filled&&!s->config[31])continue;
  unsigned id=s->config[30]?battery_order[i]:top_row[i],led=key_to_led[id];
  const uint8_t *color=s->config+(i<filled?34:37);
  for(unsigned j=0;j<3;j++){
   unsigned input=led_input[(unsigned)color[j]*s->config[40]/255];
   V8(0x20004984+j*126+led)=input*65025>>18;
  }
 }
}
void aj_lighting(void){
 init();State *s=STATE;
 restore_overlay();
 if(s->consumer_ticks&&!--s->consumer_ticks){
  if((INPUT[0x39]|((unsigned)INPUT[0x3a]<<8))==s->consumer){INPUT[0x39]=0;INPUT[0x3a]=0;INPUT[0x1e]=1;}s->consumer=0;
 }
 /* Encoder IRQ only queues turns; main context owns rendering and actions. */
 int turns;unsigned primask;__asm volatile("mrs %0, primask\ncpsid i":"=r"(primask)::"memory");turns=s->turns;s->turns=0;__asm volatile("msr primask, %0"::"r"(primask):"memory");
 while(turns){int d=turns>0?1:-1;action(s->config+(d>0?20:16),d);turns-=d;}
 if(s->active<8&&LIGHT[4]!=s->native_mode){s->active=255;s->config[5]=255;}
 unsigned advanced=advance_clock();
 if(s->active==255){NATIVE(0x7910)();battery_overlay();return;}
 for(unsigned id=0;id<128;id++)if((uint16_t)(s->frame-s->pressed_at[id])>100)s->pressed[id>>3]&=~(1u<<(id&7));
 if(s->has_key&&(uint16_t)(s->frame-s->pressed_at[s->last_key])>100)s->has_key=0;
 if(advanced||s->dirty){render();s->dirty=0;}
 battery_overlay();
}
static unsigned execute(unsigned op,const uint8_t *p,unsigned n,uint8_t *r){
 State *s=STATE;
 switch(op){
 case 0:
  if(n)return 1;copy(r,(const uint8_t *)"AJKS",4);r[4]=1;r[5]=8;r[6]=s->active;r[7]=s->config[6];r[8]=s->paused;
  for(unsigned i=0;i<8;i++)if(newest(SCENES+1024*i,0))r[9]|=1u<<i;
  copy(r+16,aj_build_id,8);
  r[10]=s->config[7];r[11]=1;
  for(unsigned i=0;i<4;i++){r[24+i]=s->frame>>(8*i);r[28+i]=s->last_ms>>(8*i);}
  copy(r+32,(const uint8_t *)"CLK2",4);r[36]=1;r[37]=20;
  r[38]=22;r[39]=1;r[40]=V8(0x2000012e);r[41]=s->battery_shown;
  return 0;
 case 1:{
  if(n!=2||p[0]>=8||p[1]>=16)return 1;
  const uint8_t *page=newest(SCENES+1024*p[0],0);if(!page)return 2;
  copy(r,page+32*p[1],32);return 0;
 }
 case 2:case 7:
  if((op==2&&(n!=1||p[0]>=8))||(op==7&&n))return 1;
  zero(s->staging,512);s->chunks=0;s->staging_kind=op==2?1:2;s->staging_slot=op==2?p[0]:0;return 0;
 case 3:case 8:{
  unsigned offset=op==3?1:0;
  if(n!=33+offset||s->staging_kind!=(op==3?1:2)||p[offset]>=16||(op==3&&p[0]!=s->staging_slot))return 1;
  copy(s->staging+32*p[offset],p+offset+1,32);s->chunks|=1u<<p[offset];return 0;
 }
 case 4:case 9:{
  int config=op==9;
  if((config?n!=0:(n!=1||p[0]!=s->staging_slot))||s->chunks!=65535||s->staging_kind!=(config?2:1)||!is_page(s->staging,config))return 1;
  if(config && s->staging[5]<8 && !newest(SCENES+1024*s->staging[5],0))return 2;
  unsigned addr=config?CONFIG:SCENES+1024*s->staging_slot;
  if(!store(addr,s->staging,config))return 3;
  s->staging_kind=0;
  if(config)copy(s->config,s->staging,512);
  load_scene();return 0;
 }
 case 5:
  if(n!=1||(p[0]!=255&&p[0]>=8))return 1;
  if(p[0]<8&&!newest(SCENES+1024*p[0],0))return 2;
  s->config[5]=p[0];load_scene();if(p[0]==255)NATIVE(0x1984)();return 0;
 case 6:
  if(n!=1||p[0]>=16)return 1;
  wr16(s->config+510,crc(s->config,510));copy(r,s->config+32*p[0],32);return 0;
 case 10:{
  if(n!=1||p[0]>=8)return 1;
  zero(s->staging,512);s->staging_kind=0;
  for(unsigned b=0;b<2;b++)((void (*)(const void *,unsigned,unsigned))0x5cb9)(s->staging,512,SCENES+1024*p[0]+b*512);
  if(newest(SCENES+1024*p[0],0))return 3;
  if(s->active==p[0]){s->config[5]=255;load_scene();NATIVE(0x1984)();}return 0;
 }
 case 11:
  if(n)return 1;return store(CONFIG,s->config,1)?0:3;
 default:return 4;
 }
}
void aj_usb(void){
 init();uint8_t *q=(uint8_t *)0x200052cc,*r=q-64;
 if(!V8(0x2000022f)||q[0]!=0xaa||q[1]!=0x70||q[2]!='A'||q[3]!='K'||q[4]!=1){
  /* Native effect selection explicitly relinquishes custom rendering. */
  if(V8(0x2000022f)&&q[0]==0xaa&&q[1]==0x23){STATE->active=255;STATE->config[5]=255;}
  NATIVE(0xea90)();return;
 }
 V8(0x2000022f)=0;zero(r,64);r[0]=0x55;r[1]=0x70;r[2]='K';r[3]='S';r[4]=1;r[5]=q[5];r[6]=q[6];
 r[7]=(q[7]>48||crc(q,62)!=rd16(q+62))?1:execute(q[5],q+8,q[7],r+8);
 wr16(r+62,crc(r,62));V8(0x2000022e)=1;
}

/* AJ159 OLED104 extension. Only the manufacturer's x5 image is patched.
 * All addresses refer to that pinned image, not to a generic YC3121 board.
 * Display and flash drivers remain the original executable instructions.
 */
#include <stdint.h>
#include <stddef.h>

#define PTR(type, address) ((type)(uintptr_t)(address))
#define U8(address) (*PTR(volatile uint8_t *, address))
#define U32(address) (*PTR(volatile uint32_t *, address))
#define CONFIG_SIZE 48
#define COOKIE 0x36545342u
#define PAGE_SIZE 48
#define ASSET_BASE 0x2005cu
#define GIF_LIMIT 218u

typedef struct {
    uint8_t config[CONFIG_SIZE];
    uint8_t telemetry[48];
    uint8_t hour, minute, fresh, second;
    uint8_t kind, asset, asset_frame, day, month, year[2], options[10];
    uint32_t timer_elapsed, timer_duration;
    uint8_t timer_mode, timer_running;
    uint8_t history[32], history_count;
} Frame;

typedef struct {
    uint32_t cookie;
    uint8_t config[CONFIG_SIZE];
    uint8_t stored_config[CONFIG_SIZE];
    uint8_t telemetry[48];
    uint32_t received_at;
    uint32_t drawn_at;
    uint8_t dirty, received, config_loaded, painted_valid;
    Frame painted;
    uint8_t pages[48], stored_pages[48];
    uint32_t schedule_second, schedule_elapsed, media_at, native_pwm;
    uint32_t erase_address, erase_end;
    uint8_t current_page, manual_page, schedule_valid, owned;
    uint8_t erase_slot, erase_progress, erase_initialized, pages_loaded;
    uint8_t button_presses;
    uint8_t timer_mode, timer_running, timer_valid;
    uint32_t timer_elapsed, timer_duration, timer_second;
    uint8_t history[32], history_count;
} State;
_Static_assert(sizeof(State) <= 512, "Screen state exceeds reserved RAM");
#define STATE PTR(volatile State *, 0x2c000)
static uint32_t lock(void);
static int valid_composition(unsigned packed);
static void unlock(uint32_t mask);

static void copy_bytes(void *to, const void *from, uint32_t count) {
    uint8_t *d = to; const uint8_t *s = from;
    while (count--) *d++ = *s++;
}
void __aeabi_memcpy(void *to, const void *from, size_t n) { copy_bytes(to, from, n); }
void __aeabi_memcpy4(void *to, const void *from, size_t n) { copy_bytes(to, from, n); }
void __aeabi_memclr(void *to, size_t n) { uint8_t *p = to; while (n--) *p++ = 0; }
void __aeabi_memclr4(void *to, size_t n) { __aeabi_memclr(to,n); }
static uint32_t divide(uint32_t a, uint32_t b) {
    uint32_t result = 0, bit = 1;
    while (b <= (a >> 1) && !(b & 0x80000000u)) { b <<= 1; bit <<= 1; }
    while (bit) { if (a >= b) { a -= b; result |= bit; } b >>= 1; bit >>= 1; }
    return result;
}
static uint16_t get16(const uint8_t *p) { return p[0] | ((uint16_t)p[1] << 8); }
static uint32_t get32(const uint8_t *p) {
    return get16(p) | ((uint32_t)get16(p + 2) << 16);
}
static uint16_t crc16(const uint8_t *p, unsigned n) {
    uint16_t c = 0xffff;
    while (n--) {
        c ^= (uint16_t)*p++ << 8;
        for (unsigned i = 0; i < 8; ++i) c = (c << 1) ^ ((c & 0x8000) ? 0x1021 : 0);
    }
    return c;
}
static void seal(uint8_t *p) { uint16_t c = crc16(p, 46); p[46] = c; p[47] = c >> 8; }
static int valid_config(const uint8_t *c) {
    if (crc16(c, 46) != get16(c + 46) || c[0] > 127 || c[1] < 1 || c[1] > 3
        || c[8] < 16 || c[8] > 124 || c[9] < 2 || c[9] > 12
        || c[10] < 1 || c[10] > 2 || c[11] < 1 || c[11] > 100
        || c[23] > 20 || c[44]>3 || c[45]>180) return 0;
    const uint8_t positions[] = {2,3,4,5,6,7,21,22};
    for (unsigned i = 0; i < sizeof positions; ++i) if (c[positions[i]] > 127) return 0;
    for (unsigned i = 0; i < 20; ++i) {
        if (i < c[23]) { if (c[24+i] < 32 || c[24+i] > 126) return 0; }
        else if (c[24+i]) return 0;
    }
    return 1;
}
static int valid_pages(const uint8_t *p) {
    if (crc16(p,46)!=get16(p+46) || p[0]>7 || p[1]<1 || p[1]>8 || p[2]>=p[1]
        || p[3]<1 || p[3]>3 || p[4]>127 || p[5]>127 || p[6]>1 || p[7]>1
        || p[40]<1 || p[40]>3 || p[41]>127 || p[42]>127 || p[43]>127 || p[44]>127 || p[45]) return 0;
    unsigned enabled=0;
    for (unsigned i=0;i<8;++i) {
        const uint8_t *e=p+8+i*4;
        if (i>=p[1]) { if (e[0] || e[1] || e[2] || e[3]) return 0; continue; }
        unsigned kind=e[0]&127, duration=get16(e+2);
        if(kind>=13&&kind<=24){if(!e[1]||!valid_composition(duration))return 0;}
        else if (kind>12 || (kind==4 ? e[1]>5 : e[1]!=0) || duration<1 || duration>3600) return 0;
        if (!(e[0]&128)) ++enabled;
    }
    return enabled!=0;
}
static unsigned asset_frames(unsigned slot) {
    unsigned n=U8(ASSET_BASE+slot+0x1f);
    return slot==5 && n>GIF_LIMIT ? GIF_LIMIT : n;
}
static unsigned asset_mask(void) {
    unsigned mask=0;
    for (unsigned i=0;i<6;++i)
        if (U8(ASSET_BASE+i+0x25) && asset_frames(i)) mask|=1u<<i;
    return mask;
}
static void initialize(void) {
    uint32_t mask=lock();
    if (STATE->cookie == COOKIE) { unlock(mask); return; }
    uint8_t *p = PTR(uint8_t *, 0x2c000);
    for (unsigned i = 0; i < sizeof(State); ++i) p[i] = 0;
    uint8_t c[48] = {62,1,50,112,6,32,6,67,116,7,1,40,
        237,244,248,13,20,31,48,220,190,6,8,9,
        'P','C',' ','/',' ','L','I','V','E'};
    seal(c);
    copy_bytes((void *)STATE->config, c, 48);
    copy_bytes((void *)STATE->stored_config, c, 48);
    uint8_t pages[48]={6,1,0,3,19,45,1,1,0,0,10,0};
    pages[40]=3;pages[41]=6;pages[42]=42;pages[43]=6;pages[44]=42;seal(pages);
    copy_bytes((void *)STATE->pages,pages,48);
    copy_bytes((void *)STATE->stored_pages,pages,48);
    STATE->cookie = COOKIE;
    STATE->dirty = 1;
    unlock(mask);
}
static uint32_t lock(void) {
    uint32_t mask;
    __asm volatile("mrs %0, primask\ncpsid i" : "=r"(mask) :: "memory");
    return mask;
}
static void unlock(uint32_t mask) { __asm volatile("msr primask, %0" :: "r"(mask) : "memory"); }

/* Font drawn here is a small hand-specified geometric 5x7 alphabet. */
#include "font.h"
static uint8_t bits(uint8_t ch, unsigned y) {
    if (ch >= '0' && ch <= '9') return digits[ch-'0'][y];
    if (ch >= 'a' && ch <= 'z') ch -= 32;
    if (ch >= 'A' && ch <= 'Z') return letters[ch-'A'][y];
    if (ch == ':') return y == 2 || y == 4 ? 4 : 0;
    if (ch == '.') return y == 6 ? 4 : 0;
    if (ch == '/') return (uint8_t)(1u << divide(y*4,6));
    if (ch == '-') return y == 3 ? 14 : 0;
    if (ch == '%') { const uint8_t p[7] = {25,25,2,4,8,19,19}; return p[y]; }
    return 0;
}
static uint16_t color(const uint8_t *p) {
    return ((uint16_t)(p[0] >> 3) << 11) | ((uint16_t)(p[1] >> 2) << 5) | (p[2] >> 3);
}
/* Compose complete opaque scanlines before touching the LCD. Two 256-byte
 * rows and the previous scene model replace a 32 KiB framebuffer. */
typedef struct { uint16_t *pixels; unsigned y; } Row;
static void rect(Row *row, unsigned x, unsigned y, unsigned w, unsigned h, uint16_t c) {
    if (x >= 128 || y >= 128 || !w || !h || row->y < y || row->y-y >= h) return;
    if (w > 128-x) w = 128-x;
    for (unsigned i=0; i<w; ++i) row->pixels[x+i] = c;
}
static void text(Row *row, const char *p, unsigned x, unsigned y, unsigned scale, uint16_t c) {
    if (row->y < y || row->y-y >= 7*scale) return;
    unsigned font_y=divide(row->y-y,scale);
    while (*p && x < 128) {
        uint8_t b = bits((uint8_t)*p, font_y);
        for (unsigned col=0; col<5; ++col)
            if (b & (16u >> col)) rect(row,x+col*scale,row->y,scale,1,c);
        ++p; x += 6*scale;
    }
}
static char *number(char *p, uint32_t n) {
    char reverse[10]; unsigned count = 0;
    do { uint32_t q = divide(n,10); reverse[count++] = '0' + n-q*10; n = q; } while (n);
    while (count) *p++ = reverse[--count];
    *p = 0; return p;
}
static void metric(Row *row, const uint8_t *c, unsigned x, unsigned y, const char *label,
                   uint16_t value, int fresh, uint16_t ink, uint16_t accent) {
    text(row,label,x,y,c[10],ink);
    if (c[0] & 4) {
        char out[8];
        if (fresh) { char *p = number(out,divide(value,10)); *p++ = '%'; *p = 0; }
        else { out[0] = '-'; out[1] = '-'; out[2] = 0; }
        text(row,out,x+30*c[10],y,c[10],accent);
    }
    if (c[0] & 8) {
        unsigned by = y + 7*c[10] + 3;
        rect(row,x,by,c[8],c[9],ink);
        rect(row,x+1,by+1,c[8]-2,c[9]-2,color(c+15));
        if (fresh) rect(row,x+1,by+1,divide((c[8]-2)*value,1000),c[9]-2,accent);
    }
}
static void memory_label(char *out,const uint8_t *t,int fresh) {
    if (!fresh) { const char empty[]="NO DATA";copy_bytes(out,empty,sizeof empty);return; }
    uint32_t a=divide(get32(t+4)*10,1024),b=divide(get32(t+8)*10,1024);
    uint32_t aq=divide(a,10),bq=divide(b,10);char *p=number(out,aq);
    *p++='.';*p++='0'+a-aq*10;*p++='/';p=number(p,bq);
    *p++='.';*p++='0'+b-bq*10;*p++='G';*p++='I';*p++='B';*p=0;
}
static void clock_label(char *out,const Frame *f) {
    unsigned h=f->hour,m=f->minute;
    out[0]='0'+divide(h,10);out[1]='0'+h-divide(h,10)*10;out[2]=':';
    out[3]='0'+divide(m,10);out[4]='0'+m-divide(m,10)*10;out[5]=0;
}
#include "modern.h"
#include "screens_extra.h"
#include "screens_game.h"
#include "screens_metrics.h"
#include "screens_composition.h"
static void compose(Row *row, const Frame *frame) {
    const uint8_t *c=frame->config, *t=frame->telemetry;
    int fresh=frame->fresh;
    uint16_t ink=color(c+12), background=color(c+15), accent=color(c+18);
    rect(row,0,0,128,128,background);
    if(frame->kind>=13&&frame->kind<=24){composition_screen(row,frame,ink,accent,background);return;}
    if (c[0] & 64) { rect(row,0,0,128,1,accent); rect(row,0,127,128,1,accent); }
    if(frame->kind>=9 && frame->kind<=12){metrics_screen(row,frame,ink,accent,background);return;}
    if(frame->kind==0 && c[44]==3){game_screen(row,frame,ink,accent);return;}
    if(c[44] && frame->kind>=1 && frame->kind<=3){modern_detail(row,frame,ink,accent);return;}
    if((frame->kind>=5 && frame->kind<=8) || (frame->kind==0 && c[44])) {
        extra_screen(row,frame,ink,accent,background);return;
    }
    if (c[0]&32) {
        char title[21];copy_bytes(title,c+24,c[23]);title[c[23]]=0;
        const char *label=frame->kind==1 ? "CPU LOAD" : frame->kind==2 ? "MEMORY" : frame->kind==3 ? "TIME" : title;
        text(row,label,c[21],c[22],1,accent);
    }
    if (frame->kind==255) { text(row,"EMPTY SLOT",6,56,1,ink);return; }
    if (frame->kind==3) {
        char clock[6];clock_label(clock,frame);
        text(row,clock,frame->options[1],frame->options[2],frame->options[0],ink);
        if (frame->options[3]) {
            char date[16];unsigned year=get16(frame->year);char *p=date;
            if (year<2000 || year>2099 || !frame->month || !frame->day) {
                const char empty[]="NO DATE";copy_bytes(date,empty,sizeof empty);
            } else {
                p=number(p,year);*p++='.';*p++='0'+divide(frame->month,10);*p++='0'+frame->month-divide(frame->month,10)*10;
                *p++='.';*p++='0'+divide(frame->day,10);*p++='0'+frame->day-divide(frame->day,10)*10;*p=0;
            }
            text(row,date,frame->options[1],frame->options[2]+7*frame->options[0]+12,1,accent);
        }
        if(frame->options[4]) {
            char seconds[7]={'S','E','C',' ','0'+divide(frame->second,10),'0'+frame->second-divide(frame->second,10)*10,0};
            text(row,seconds,frame->options[1],frame->options[2]+7*frame->options[0]+26,1,accent);
        }
        return;
    }
    if (frame->kind==1 || frame->kind==2) {
        unsigned index=frame->kind-1,scale=frame->options[5];
        unsigned x=frame->options[6+index*2],y=frame->options[7+index*2];
        unsigned value=get16(t+index*2);char out[8];
        if (fresh) { char *p=number(out,divide(value,10));*p++='%';*p=0; }
        else { out[0]='-';out[1]='-';out[2]=0; }
        text(row,out,x,y,scale,accent);
        if (c[0]&8) {
            unsigned by=y+7*scale+6;
            rect(row,x,by,c[8],c[9],ink);rect(row,x+1,by+1,c[8]-2,c[9]-2,background);
            if (fresh)rect(row,x+1,by+1,divide((c[8]-2)*value,1000),c[9]-2,accent);
        }
        if (index && (c[0]&16)) {
            char memory[28];memory_label(memory,t,fresh);
            text(row,memory,x,y+7*scale+c[9]+12,1,ink);
        }
        return;
    }
    metric(row,c,c[4],c[5],"CPU",get16(t),fresh,ink,accent);
    metric(row,c,c[6],c[7],"RAM",get16(t+2),fresh,ink,accent);
    if (c[0] & 16) {
        char out[28], *p = out;
        if (fresh) {
            uint32_t a = divide(get32(t+4)*10,1024), b = divide(get32(t+8)*10,1024);
            uint32_t aq = divide(a,10), bq = divide(b,10);
            p=number(p,aq); *p++='.'; *p++='0'+a-aq*10; *p++='/';
            p=number(p,bq); *p++='.'; *p++='0'+b-bq*10; *p++='G'; *p++='I'; *p++='B'; *p=0;
        } else { const char empty[]="NO DATA"; copy_bytes(out,empty,sizeof empty); }
        text(row,out,c[6],c[7]+7*c[10]+c[9]+6,1,ink);
    }
    if (c[0] & 2) {
        unsigned h=frame->hour, m=frame->minute;
        char clock[6] = {'0'+divide(h,10),'0'+h-divide(h,10)*10,':',
            '0'+divide(m,10),'0'+m-divide(m,10)*10,0};
        text(row,clock,c[2],c[3],c[1],ink);
    }
}
static int equal(const uint8_t *a, const uint8_t *b, unsigned n) {
    while (n--) if (*a++ != *b++) return 0;
    return 1;
}

static int page_eligible(const uint8_t *p,unsigned i,int fresh,unsigned assets) {
    const uint8_t *e=p+8+i*4;unsigned kind=e[0]&127;
    if (e[0]&128) return 0;
    if ((p[0]&4) && !fresh && (kind<3 || (kind>=9 && kind<=12))) return 0;
    if((p[0]&4)&&!fresh&&kind>=13&&kind<=24){unsigned metrics=get16(e+2),clock=0;for(unsigned j=0;j<4;++j)clock|=((metrics>>(j*4))&15)==11;if(!clock)return 0;}
    return kind!=4 || (assets&(1u<<e[1]));
}
static void select_page(Frame *f,uint32_t tick) {
    uint8_t p[48];copy_bytes(p,(const void *)STATE->pages,48);
    f->kind=0;
    if (!(p[0]&1)) return;
    copy_bytes(f->options,p+3,5);copy_bytes(f->options+5,p+40,5);
    unsigned second=f->second;
    uint32_t now=f->hour*3600u+f->minute*60u+second;
    if (STATE->schedule_valid) {
        uint32_t last=STATE->schedule_second,delta=now>=last ? now-last : 86400u-last+now;
        if (delta<=60) STATE->schedule_elapsed+=delta;else STATE->schedule_elapsed=0;
    }
    STATE->schedule_valid=1;STATE->schedule_second=now;
    unsigned index=STATE->current_page;
    if(index>=p[1])index=p[2];
    unsigned presses=STATE->button_presses;
    STATE->button_presses=0;
    if(presses) {
        unsigned assets=asset_mask();
        while(presses--) {
            unsigned n=0;
            do {index=index+1<p[1] ? index+1 : 0;++n;}
            while(n<p[1] && !page_eligible(p,index,f->fresh,assets));
        }
        STATE->schedule_elapsed=0;STATE->media_at=tick;
        STATE->manual_page=page_eligible(p,index,f->fresh,assets);
    }
    unsigned wire_kind=p[8+index*4]&127;
    unsigned duration=wire_kind>=13&&wire_kind<=24 ? p[9+index*4] : get16(p+8+index*4+2);
    if ((p[0]&2) && STATE->schedule_elapsed>=duration) {
        index=index+1<p[1] ? index+1 : 0;
        STATE->schedule_elapsed=0;STATE->manual_page=0;
    }
    unsigned assets=asset_mask();
    if (!STATE->manual_page) {
        unsigned n=0;
        while(n<p[1] && !page_eligible(p,index,f->fresh,assets)) {
            index=index+1<p[1] ? index+1 : 0;++n;
        }
        if(n==p[1]) { f->kind=3;return; }
    }
    if (index!=STATE->current_page) { STATE->schedule_elapsed=0;STATE->media_at=tick; }
    STATE->current_page=index;
    const uint8_t *e=p+8+index*4;f->kind=e[0]&127;f->asset=e[1];
    if(f->kind>=13&&f->kind<=24){f->options[0]=e[2];f->options[1]=e[3];f->asset=0;if(f->kind!=13&&f->kind!=19)f->asset_frame=divide(tick,100);}
    if(f->kind==4) {
        if(!(assets&(1u<<f->asset))) { f->kind=255;return; }
        if(f->asset==5) {
            unsigned count=asset_frames(5),delay=U8(ASSET_BASE+5+0x19);
            if(!delay)delay=1;
            unsigned n=divide(tick-STATE->media_at,delay);
            f->asset_frame=n-divide(n,count)*count;
        }
    }
}
/* Rear screen button arrives as a validated outer UART opcode 0x42.
 * Keep the original dispatcher/ACK; defer custom navigation to the main loop.
 * The original 0x44 long-press handler is deliberately left intact. */
__attribute__((used)) void screen_button_hook(void) {
    initialize();
    if(!(STATE->pages[0]&1)) {PTR(void (*)(void),0x01013eb5)();return;}
    uint32_t mask=lock();
    if(U8(0x2a141) && !U32(ASSET_BASE+0x60) && STATE->button_presses<255)
        ++STATE->button_presses;
    unlock(mask);
}
static void prune_asset(uint8_t *p,unsigned slot) {
    unsigned count=0;
    for(unsigned i=0;i<p[1];++i) {
        uint8_t *e=p+8+i*4;
        if ((e[0]&127)==4 && e[1]==slot)continue;
        if(count!=i)copy_bytes(p+8+count*4,e,4);
        ++count;
    }
    if(!count) {p[8]=3;p[9]=0;p[10]=10;p[11]=0;count=1;}
    p[1]=count;if(p[2]>=count)p[2]=0;
    for(unsigned i=count*4+8;i<40;++i)p[i]=0;
    /* Deleting the only enabled entry must retain a functional clock page. */
    unsigned enabled=0;for(unsigned i=0;i<count;++i)enabled|=!(p[8+i*4]&128);
    if(!enabled)p[8]&=127;
    seal(p);
}
static void erase_service(void) {
    unsigned queued=STATE->erase_slot;if(!queued)return;
    if(!STATE->erase_initialized) {
        uint32_t mask=lock();
        unsigned slot=queued-1,count=asset_frames(slot);
        uint32_t address=slot==5 ? 0x101000u : 0x7d1000u+slot*0x8000u;
        STATE->erase_address=address;STATE->erase_end=address+(slot==5 ? count : 1)*0x8000u;
        if(!count || STATE->erase_end>(slot==5 ? 0x7d1000u : 0x800000u)) {STATE->erase_slot=0;unlock(mask);return;}
        U8(ASSET_BASE+slot+0x25)=0;U8(ASSET_BASE+slot+0x1f)=0;
        uint8_t p[48];copy_bytes(p,(const void *)STATE->pages,48);prune_asset(p,slot);
        copy_bytes((void *)STATE->pages,p,48);
        copy_bytes(p,(const void *)STATE->stored_pages,48);prune_asset(p,slot);
        copy_bytes((void *)STATE->stored_pages,p,48);
        STATE->current_page=0;STATE->manual_page=0;STATE->schedule_elapsed=0;
        STATE->painted_valid=0;U8(0x2a142)=1;STATE->erase_initialized=1;unlock(mask);return;
    }
    /* Persist invalid metadata first. A power cut cannot resurrect a half-erased asset. */
    if(U8(0x2a142) || U8(0x2a143))return;
    uint32_t address=STATE->erase_address;
    if(address<STATE->erase_end) {
        PTR(void (*)(uint32_t),0x01015fa9)(address);
        STATE->erase_address=address+0x1000u;
    }
    uint32_t base=queued==6 ? 0x101000u : 0x7d1000u+(queued-1)*0x8000u;
    STATE->erase_progress=divide((STATE->erase_address-base)*100u,STATE->erase_end-base);
    if(STATE->erase_address>=STATE->erase_end) {STATE->erase_slot=0;STATE->erase_initialized=0;STATE->erase_progress=100;}
}
static void return_native(void) {
    if(!STATE->owned)return;
    /* Native drawing is incremental too: invalidate its full-page cache and
     * remove the old custom frame before handing display ownership back. */
    PTR(void (*)(unsigned,unsigned,unsigned,unsigned,unsigned),0x01010953)(0,0,128,128,0);
    U32(0x2a124)=0xffffffffu;U8(0x2a14b)=1;U8(0x2a1a0)=255;U8(0x2a1a1)=1;
    PTR(void (*)(unsigned),0x010108a7)(STATE->native_pwm);
    STATE->owned=0;STATE->painted_valid=0;
}
static void timer_service(Frame *frame) {
    uint32_t now=frame->hour*3600u+frame->minute*60u+frame->second;
    if(STATE->timer_valid && STATE->timer_running) {
        uint32_t last=STATE->timer_second,delta=now>=last ? now-last : 86400u-last+now;
        if(delta<=60) {
            STATE->timer_elapsed+=delta;
            if(STATE->timer_mode==1 && STATE->timer_elapsed>=STATE->timer_duration) {
                STATE->timer_elapsed=STATE->timer_duration;STATE->timer_running=0;
            }
        }
    }
    STATE->timer_second=now;STATE->timer_valid=1;
    frame->timer_elapsed=STATE->timer_elapsed;frame->timer_duration=STATE->timer_duration;
    frame->timer_mode=STATE->timer_mode;frame->timer_running=STATE->timer_running;
}
/* This runs in the original main loop, never in the serial interrupt. */
__attribute__((used)) int screen_tick(void) {
    initialize();
    erase_service();
    Frame frame={0}, previous;
    uint32_t mask=lock();
    copy_bytes(frame.config,(const void *)STATE->config,48);
    copy_bytes(frame.telemetry,(const void *)STATE->telemetry,48);
    copy_bytes(frame.history,(const void *)STATE->history,32);frame.history_count=STATE->history_count;
    uint32_t tick=U32(0x2a138), age=tick-STATE->received_at;
    frame.fresh=STATE->received && age<2700;
    frame.hour=U8(0x2a14e); frame.minute=U8(0x2a14d);
    frame.second=U8(0x2a14c);if(frame.second>59)frame.second=0;
    if (frame.hour>23) frame.hour=0;
    if (frame.minute>59) frame.minute=0;
    frame.day=U8(0x2a14f);frame.month=U8(0x2a150);
    unsigned year=U32(0x2a154);frame.year[0]=year;frame.year[1]=year>>8;
    timer_service(&frame);
    select_page(&frame,tick);
    if (!(frame.config[0]&1) && !(STATE->pages[0]&1)) { unlock(mask);return_native();return 0; }
    if(!U8(0x2a141) || U32(ASSET_BASE+0x60)) {unlock(mask);return_native();return 0;}
    if(!STATE->owned) {STATE->native_pwm=U32(0xf0c40);STATE->owned=1;STATE->painted_valid=0;}
    int valid=STATE->painted_valid;
    copy_bytes(&previous,(const void *)&STATE->painted,sizeof previous);
    /* Sequence/CRC and seconds are not displayed. Identical scenes do no I/O. */
    if (valid && equal(frame.config,previous.config,48)
        && equal(frame.telemetry,previous.telemetry,12)
        && equal(frame.telemetry+16,previous.telemetry+16,14)
        && equal(frame.telemetry+32,previous.telemetry+32,14)
        && equal(&frame.hour,&previous.hour,sizeof(Frame)-offsetof(Frame,hour))) { STATE->dirty=0; unlock(mask); return 1; }
    if (tick-STATE->drawn_at<(frame.kind==4 ? 1u : 80u) && STATE->drawn_at) { unlock(mask); return 1; }
    STATE->drawn_at=tick; STATE->dirty=0;
    unlock(mask);
    if (!valid || frame.config[11]!=previous.config[11])
        PTR(void (*)(unsigned),0x010108a7)(divide(65535u*frame.config[11],100));
    if(frame.kind==4) {
        if(!valid || previous.kind!=4 || previous.asset!=frame.asset || previous.asset_frame!=frame.asset_frame) {
            uint32_t old_slot=U32(ASSET_BASE+0x2c),old_frame=U32(ASSET_BASE+0x3c);
            U32(ASSET_BASE+0x2c)=frame.asset;U32(ASSET_BASE+0x3c)=frame.asset_frame;
            PTR(void (*)(unsigned,unsigned,unsigned,unsigned),0x010111dd)(0,0,128,128);
            U32(ASSET_BASE+0x2c)=old_slot;U32(ASSET_BASE+0x3c)=old_frame;
        }
    } else {
    if(valid && previous.kind==4)valid=0;
    uint16_t next[128], old[128];
    for (unsigned y=0; y<128; ++y) {
        Row row={next,y}; compose(&row,&frame);
        if (valid) { Row before={old,y}; compose(&before,&previous); }
        unsigned first=0, end=128;
        if (valid) {
            while (first<end && next[first]==old[first]) ++first;
            while (end>first && next[end-1]==old[end-1]) --end;
        }
        if (first<end) {
            /* Native image blit writes big-endian RGB565 directly to panel GRAM.
             * Every transmitted pixel is final: no background erase phase. */
            for (unsigned x=first; x<end; ++x) next[x]=(uint16_t)((next[x]<<8)|(next[x]>>8));
            PTR(void (*)(unsigned,unsigned,unsigned,unsigned,const void *),0x01010c61)
                (first,y,end-first,1,next+first);
        }
        PTR(void (*)(void),0x01015bfd)(); /* Original watchdog service. */
    }
    }
    mask=lock();
    copy_bytes((void *)&STATE->painted,&frame,sizeof frame);
    STATE->painted_valid=1;
    unlock(mask);
    return 1;
}

__attribute__((naked,used)) void screen_main_hook(void) {
    __asm volatile(
        "push {r4,lr}\nldr r3, =0x0101602d\nblx r3\nbl screen_tick\n"
        "cmp r0,#0\nbeq 1f\npop {r4}\npop {r1}\nldr r0, =0x010104c5\nbx r0\n"
        "1: pop {r4,pc}\n");
}
static void response(uint8_t status) {
    uint8_t r[64] = {0x2c,'A','J','S','C',1,0,0}; r[6]=status;
    uint8_t sum=0; for (unsigned i=0;i<7;++i) sum+=r[i]; r[7]=255-sum;
    copy_bytes(r+8,(const void *)STATE->config,48);
    copy_bytes(r+56,(const void *)STATE->telemetry,4);
    copy_bytes(r+60,(const void *)(STATE->telemetry+12),2);
    r[62]=STATE->received; r[63]=255;
    PTR(void (*)(const void *,const void *),0x010139ab)(PTR(void *,0x2a577),r);
}
static void pages_response(uint8_t status) {
    uint8_t r[64]={0x2c,'A','J','P','G',1,0,0};r[6]=status;
    uint8_t sum=0;for(unsigned i=0;i<7;++i)sum+=r[i];r[7]=255-sum;
    copy_bytes(r+8,(const void *)STATE->pages,48);
    r[56]=STATE->current_page;r[57]=STATE->painted.kind;r[58]=asset_mask();r[59]=asset_frames(5);
    r[60]=STATE->erase_slot;r[61]=STATE->erase_progress;r[62]=U8(0x2a142)||U8(0x2a143);r[63]=255;
    PTR(void (*)(const void *,const void *),0x010139ab)(PTR(void *,0x2a577),r);
}
static void timer_response(uint8_t status) {
    uint8_t r[64]={0x2c,'A','J','T','M',1,0,0};r[6]=status;
    uint8_t sum=0;for(unsigned i=0;i<7;++i)sum+=r[i];r[7]=255-sum;
    r[8]=STATE->timer_mode;r[9]=STATE->timer_running;
    r[10]=STATE->timer_mode==1 && STATE->timer_elapsed>=STATE->timer_duration;
    uint32_t a=STATE->timer_elapsed,b=STATE->timer_duration;
    for(unsigned i=0;i<4;++i){r[12+i]=a>>(i*8);r[16+i]=b>>(i*8);}
    r[63]=255;
    PTR(void (*)(const void *,const void *),0x010139ab)(PTR(void *,0x2a577),r);
}
__attribute__((used)) void screen_usb_hook(uint32_t unused, const uint8_t *r) {
    (void)unused;
    initialize();
    if (STATE->erase_slot && (r[0]==0xa5 || r[0]==0x25)) {
        uint8_t busy[64]={0xa5,0};
        PTR(void (*)(const void *,const void *),0x010139ab)(PTR(void *,0x2a577),busy);return;
    }
    if (r[0] != 0x2c || r[8]!='A' || r[9]!='J' || r[10]!='S' || r[11]!='C') {
        copy_bytes(PTR(void *,0x2b638),r,64);
        PTR(void (*)(void),0x01013ccf)(); return;
    }
    uint8_t status=0, sum=0;
    for (unsigned i=0;i<8;++i) sum+=r[i];
    if (sum!=255 || r[12]!=1 || r[15]) { response(1); return; }
    uint8_t action=r[13], size=r[14];
    if(action==11 || action==12) {
        if(action==12 && size==0){timer_response(0);return;}
        if(action!=11 || size!=1 || r[16]>5)status=2;
        else {
            unsigned cmd=r[16];
            if(cmd==0 || cmd==1) {
                STATE->timer_mode=cmd+1;STATE->timer_running=1;STATE->timer_elapsed=0;
                STATE->timer_duration=cmd==0 ? (STATE->config[45] ? STATE->config[45] : 25)*60u : 0;
                STATE->timer_valid=0;
                for(unsigned i=0;i<STATE->pages[1];++i)if(STATE->pages[8+i*4]==(cmd==0 ? 7 : 8)) {
                    STATE->current_page=i;STATE->manual_page=1;STATE->schedule_elapsed=0;break;
                }
            } else if(cmd==2) STATE->timer_running=0;
            else if(cmd==3) {
                if(!STATE->timer_mode || (STATE->timer_mode==1 && STATE->timer_elapsed>=STATE->timer_duration))status=2;
                else {STATE->timer_running=1;STATE->timer_valid=0;}
            } else if(cmd==4) {
                STATE->timer_elapsed=0;STATE->timer_running=0;
                STATE->timer_duration=STATE->timer_mode==1 ? (STATE->config[45] ? STATE->config[45] : 25)*60u : 0;
            } else {STATE->timer_mode=0;STATE->timer_running=0;STATE->timer_elapsed=0;STATE->timer_duration=0;}
        }
        timer_response(status);return;
    }
    if (action>=4 && action<=10) {
        if(action==4 && size==0) {pages_response(0);return;}
        if((action==5 || action==6) && size==48) {
            if(!valid_pages(r+16))status=2;
            else {
                copy_bytes((void *)STATE->pages,r+16,48);STATE->current_page=r[18];
                STATE->button_presses=0;
                STATE->schedule_elapsed=0;STATE->schedule_valid=0;STATE->manual_page=0;STATE->media_at=U32(0x2a138);
                if(action==6) {copy_bytes((void *)STATE->stored_pages,r+16,48);U8(0x2a142)=1;}
            }
        } else if(action==7 && size==1) {
            if(r[16]>=STATE->pages[1] || (STATE->pages[8+r[16]*4]&128))status=2;
            else {STATE->current_page=r[16];STATE->manual_page=1;STATE->schedule_elapsed=0;STATE->media_at=U32(0x2a138);}
        } else if(action==8 && size==1) {
            if(r[16]>5 || !(asset_mask()&(1u<<r[16])))status=2;
            else if(STATE->erase_slot || !U8(0x2a141) || U32(ASSET_BASE+0x60))status=3;
            else {STATE->erase_slot=r[16]+1;STATE->erase_initialized=0;STATE->erase_progress=0;}
        } else if(action==9 && size==0) {
            STATE->received=0;uint8_t zero[32]={0};copy_bytes((void *)STATE->telemetry,zero,32);
            STATE->manual_page=0;STATE->schedule_elapsed=0;
            for(unsigned i=0;i<STATE->pages[1];++i)
                if(STATE->pages[8+i*4]==3) {STATE->current_page=i;break;}
        } else if(action==10 && size==0) {
            STATE->button_presses=0;
            uint8_t c[48];copy_bytes(c,(const void *)STATE->config,48);c[0]&=~1;seal(c);copy_bytes((void *)STATE->config,c,48);
            copy_bytes(c,(const void *)STATE->pages,48);c[0]&=~1;seal(c);copy_bytes((void *)STATE->pages,c,48);STATE->received=0;
        } else status=2;
        pages_response(status);return;
    }
    if(action==13 && size==0) {
        uint8_t g[64]={0x2c,'A','J','G','M',1,0,0};uint8_t sum=0;
        for(unsigned i=0;i<7;i++)sum+=g[i];g[7]=255-sum;g[8]=1;
        g[9]='U';g[10]='I';g[11]=4;g[12]=255-g[9]-g[10]-g[11];
        copy_bytes(g+16,(const void *)STATE->telemetry,48);
        PTR(void (*)(const void *,const void *),0x010139ab)(PTR(void *,0x2a577),g);return;
    }
    if (action==0 && size==0) { response(0); return; }
    if ((action==1 || action==3) && size==48) {
        if (!valid_config(r+16)) status=2;
        else {
            copy_bytes((void *)STATE->config,r+16,48); STATE->dirty=1;
            if (action==3) {
                copy_bytes((void *)STATE->stored_config,r+16,48);
                U8(0x2a142)=1; /* Original main-loop save, not ISR flash. */
            }
        }
    } else if ((action==2 && size==16)||(action==14 && size==32)||(action==15 && size==48)) {
        if (crc16(r+16,14)!=get16(r+30) || get16(r+16)>1000 || get16(r+18)>1000
            || get32(r+24)==0 || get32(r+20)>get32(r+24) || get32(r+24)>8388608u
            || (action>=14 && (r[32]!=1 || r[33]>7 || crc16(r+16,30)!=get16(r+46)
                || ((r[33]&1) && get16(r+34)>1000)
                || ((r[33]&2) && (!get32(r+40) || get32(r+36)>get32(r+40) || get32(r+40)>1048576u))
                || ((r[33]&4) && !get16(r+44))))
            || (action==15 && (r[48]!=1 || r[49]>31 || r[60] || r[61] || crc16(r+16,46)!=get16(r+62)
                || ((r[49]&1) && ((int16_t)get16(r+50)<-100 || (int16_t)get16(r+50)>1500))
                || ((r[49]&2) && ((int16_t)get16(r+52)<-100 || (int16_t)get16(r+52)>1500))
                || ((r[49]&4) && ((int16_t)get16(r+54)<-100 || (int16_t)get16(r+54)>1500))
                || ((r[49]&8) && ((int16_t)get16(r+56)<-100 || (int16_t)get16(r+56)>1500))))) status=2;
        else {
            uint32_t tick=U32(0x2a138);
            if(!STATE->manual_page && (!STATE->received || tick-STATE->received_at>=2700)) {
                STATE->current_page=STATE->pages[2];STATE->schedule_elapsed=0;STATE->schedule_valid=0;STATE->media_at=tick;
            }
            if(!STATE->received || tick-STATE->received_at>=2700)STATE->history_count=0;
            /* One observation per sequence. A retransmission cannot create history. */
            if(!STATE->history_count || get16(r+28)!=get16((const uint8_t *)STATE->telemetry+12)) {
                unsigned count=STATE->history_count;if(count>31)count=31;
                for(unsigned i=count;i>0;--i)STATE->history[i]=STATE->history[i-1];
                unsigned gpu=action>=14 && (r[33]&1) ? divide(get16(r+34)*14,1000) : 15;
                STATE->history[0]=divide(get16(r+16)*14,1000)|(gpu<<4);STATE->history_count=count+1;
            }
            uint8_t zero[48]={0};copy_bytes((void *)STATE->telemetry,zero,48);
            copy_bytes((void *)STATE->telemetry,r+16,size);
            STATE->received_at=tick; STATE->received=1; STATE->dirty=1;
        }
    } else status=3;
    response(status);
}
__attribute__((used)) void screen_config_write(uint8_t *p,uint32_t address,uint32_t n) {
    initialize();
    if (address==0x100000 && n==256) {
        uint32_t mask=lock();
        p[128]='A';p[129]='J';p[130]='S';p[131]='C';p[132]=1;
        copy_bytes(p+133,(const void *)STATE->stored_config,48);
        p[181]='A';p[182]='J';p[183]='P';p[184]='G';p[185]=1;
        copy_bytes(p+186,(const void *)STATE->stored_pages,48);
        unlock(mask);
    }
    PTR(void (*)(void *,uint32_t,uint32_t),0x010161fd)(p,address,n);
}
__attribute__((used)) void screen_config_read(uint8_t *p,uint32_t address,uint32_t n) {
    PTR(void (*)(void *,uint32_t,uint32_t),0x01016237)(p,address,n);
    initialize();
    uint32_t mask=lock();
    if (address==0x100000 && n==256 && p[128]=='A' && p[129]=='J' && p[130]=='S'
        && p[131]=='C' && p[132]==1 && valid_config(p+133)) {
        copy_bytes((void *)STATE->stored_config,p+133,48);
        if (!STATE->config_loaded) copy_bytes((void *)STATE->config,p+133,48);
        STATE->dirty=1;
    }
    if(address==0x100000 && n==256 && p[181]=='A' && p[182]=='J' && p[183]=='P'
        && p[184]=='G' && p[185]==1 && valid_pages(p+186)) {
        copy_bytes((void *)STATE->stored_pages,p+186,48);
        if(!STATE->pages_loaded) {
            copy_bytes((void *)STATE->pages,p+186,48);STATE->current_page=p[188];
            STATE->schedule_valid=0;STATE->media_at=U32(0x2a138);
        }
    }
    STATE->pages_loaded=1;
    STATE->config_loaded=1;
    unlock(mask);
}

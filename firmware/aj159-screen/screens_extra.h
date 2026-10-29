/* Additional autonomous screens. All output uses the same opaque row compositor. */
static void point(Row *r,int x,int y,uint16_t c){rect(r,(unsigned)x,(unsigned)y,1,1,c);}
static void line(Row *r,int x,int y,int to_x,int to_y,uint16_t c){
    if((int)r->y<(y<to_y ? y : to_y) || (int)r->y>(y>to_y ? y : to_y))return;
    int dx=to_x>x ? to_x-x : x-to_x, sx=x<to_x ? 1 : -1;
    int dy=to_y>y ? y-to_y : to_y-y, sy=y<to_y ? 1 : -1,err=dx+dy;
    for(;;){point(r,x,y,c);if(x==to_x && y==to_y)break;int e=err*2;
        if(e>=dy){err+=dy;x+=sx;}if(e<=dx){err+=dx;y+=sy;}}
}
static const int8_t sine[60]={0,10,21,31,41,50,59,67,74,81,87,91,95,98,99,100,99,98,95,91,87,81,74,67,59,50,41,31,21,10,0,-10,-21,-31,-41,-50,-59,-67,-74,-81,-87,-91,-95,-98,-99,-100,-99,-98,-95,-91,-87,-81,-74,-67,-59,-50,-41,-31,-21,-10};
static int scaled(int value,unsigned radius){int n=(int)divide((value<0 ? -value : value)*radius,100);return value<0 ? -n : n;}
static void hand(Row *r,unsigned index,unsigned radius,uint16_t ink,unsigned thick){
    int x=64+scaled(sine[index],radius),y=72-scaled(sine[index+15>=60 ? index-45 : index+15],radius);
    line(r,64,72,x,y,ink);if(thick)line(r,65,72,x+1,y,ink);
}
static unsigned focus_minutes(const uint8_t *c){return c[45] ? c[45] : 25;}
static void timer_text(char *out,unsigned seconds){
    if(seconds>599999)seconds=599999;
    unsigned minutes=divide(seconds,60),s=seconds-minutes*60;char *p=out;
    if(minutes<10)*p++='0';p=number(p,minutes);*p++=':';*p++='0'+divide(s,10);*p++='0'+s-divide(s,10)*10;*p=0;
}
static void extra_screen(Row *row,const Frame *f,uint16_t ink,uint16_t accent,uint16_t background){
    const uint8_t *c=f->config;
    if(f->kind==5){
        char clock[6];clock_label(clock,f);modern_center(row,clock,8,0,accent);
        int x=43,y=0,error=-42;
        while(x>=y){
            point(row,64+x,72+y,ink);point(row,64+y,72+x,ink);point(row,64-y,72+x,ink);point(row,64-x,72+y,ink);
            point(row,64-x,72-y,ink);point(row,64-y,72-x,ink);point(row,64+y,72-x,ink);point(row,64+x,72-y,ink);
            ++y;error+=2*y-1;if(error>=0){--x;error-=2*x;}
        }
        for(unsigned i=0;i<60;i+=5){
            int dx=sine[i],dy=sine[i+15>=60 ? i-45 : i+15];
            line(row,64+scaled(dx,38),72-scaled(dy,38),64+scaled(dx,41),72-scaled(dy,41),accent);
        }
        modern_text(row,"12",56,35,3,ink);modern_text(row,"3",96,68,3,ink);
        modern_text(row,"6",60,100,3,ink);modern_text(row,"9",24,68,3,ink);
        unsigned h=f->hour-divide(f->hour,12)*12;
        hand(row,h*5+divide(f->minute,12),23,ink,1);hand(row,f->minute,33,ink,1);
        if(f->options[4])hand(row,f->second,37,accent,0);
        rect(row,62,70,5,5,accent);
        return;
    }
    if(f->kind==6){
        unsigned year=get16(f->year),month=f->month,day=f->day;
        if(year<2000 || year>2099 || month<1 || month>12 || day<1 || day>31){text(row,"NO DATE",40,58,1,ink);return;}
        const char *months[12]={"JAN","FEB","MAR","APR","MAY","JUN","JUL","AUG","SEP","OCT","NOV","DEC"};
        modern_text(row,months[month-1],7,8,0,accent);char value[12];number(value,year);modern_text(row,value,76,8,0,ink);
        const char labels[]="MTWTFSS";
        for(unsigned i=0;i<7;++i){char label[2]={labels[i],0};modern_text(row,label,7+i*17,29,0,ink);}
        unsigned yy=year-(month<3),offsets[12]={0,3,2,5,0,3,5,1,4,6,2,4};
        unsigned w=yy+divide(yy,4)-divide(yy,100)+divide(yy,400)+offsets[month-1]+1;
        w=w-divide(w,7)*7;w=w ? w-1 : 6;
        unsigned days=month==2 ? (year%4==0 ? 29 : 28) : (month==4 || month==6 || month==9 || month==11 ? 30 : 31);
        for(unsigned d=1;d<=days;++d){
            unsigned cell=w+d-1,y=46+divide(cell,7)*14,x=7+(cell-divide(cell,7)*7)*17;
            if(d==day)pill(row,x-1,y-1,16,12,accent);
            number(value,d);unsigned width=modern_width(value,3);modern_text(row,value,x+(width<14?divide(14-width,2):0),y,3,d==day ? background : ink);
        }
        return;
    }
    if(f->kind==7 || f->kind==8){
        unsigned mode=f->kind==7 ? 1 : 2,active=f->timer_mode==mode;
        unsigned duration=active ? f->timer_duration : focus_minutes(c)*60;
        unsigned seconds=active ? f->timer_elapsed : 0;
        if(mode==1)seconds=duration>seconds ? duration-seconds : 0;
        if(c[0]&32)modern_center(row,mode==1 ? "FOCUS" : "STOPWATCH",9,0,accent);
        char out[12];timer_text(out,seconds);unsigned font=modern_width(out,2)<=120?2:1;
        modern_center(row,out,43,font,ink);
        const char *status=!active ? "READY" : f->timer_running ? "RUNNING" : mode==1 && !seconds ? "DONE" : "PAUSED";
        modern_center(row,status,88,0,accent);
        if(mode==1&&duration)modern_bar(row,8,112,112,6,divide(1000*seconds,duration),1,accent,background);
        return;
    }
    modern_summary(row,f,ink,accent);
}

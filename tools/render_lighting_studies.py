"""Render offline AJKS motion studies, with a single stable GIF palette per study."""
from pathlib import Path
import json,sys
import numpy as np
from PIL import Image,ImageDraw,ImageFont

ROOT=Path(__file__).resolve().parents[1]
FONT=ImageFont.truetype('C:/Windows/Fonts/segoeui.ttf',17)
SMALL=ImageFont.truetype('C:/Windows/Fonts/segoeui.ttf',12)
ROWS=json.loads((ROOT/'assets/ak820max-layout.json').read_text(encoding='utf-8'))
def geometry(x,y,w):
    unit=w/17.5;rects=[]
    for r,row in enumerate(ROWS):
        left=.35
        for c,k in enumerate(row):
            if r==0 and c in (1,5,9,13):left+=.25
            if 1<=r<=4 and c==len(row)-1 or r==5 and k['physical']=='ArrowLeft':left+=.25
            wide=1.75 if k['physical']=='CapsLock' else k['width']
            top=.35+(0 if r==0 else 1.4+(r-1)*1.06)
            rects.append((x+left*unit,y+top*unit,x+(left+wide-.12)*unit,y+(top+.88)*unit))
            left+=wide
    assert len(rects)==82
    return rects,(x+16.35*unit,y+.79*unit),unit

def tile(study,frame,w=360,h=185):
    im=Image.new('RGB',(w,h),(18,20,25));d=ImageDraw.Draw(im)
    d.rounded_rectangle((2,2,w-3,h-3),14,fill=(25,29,37),outline=(44,50,62))
    d.text((18,12),study['look']['name'],font=FONT,fill=(240,244,250))
    kw=w-34;kh=kw*7.2/17.5;x=17;y=53+(h-60-kh)/2
    d.rounded_rectangle((x,y,x+kw,y+kh),7,fill=(36,41,50),outline=(73,81,95))
    rects,knob,unit=geometry(x,y,kw)
    colors=np.array(study['frames'][frame],dtype=np.uint8).reshape(82,3)
    for rect,color in zip(rects,colors):
        color=tuple(max(int(v),base) for v,base in zip(color,[22,26,34]))
        d.rounded_rectangle(rect,unit*.15,fill=color,outline=tuple(int(v*.7) for v in color),width=1)
    r=unit*.44;d.ellipse((knob[0]-r,knob[1]-r,knob[0]+r,knob[1]+r),fill=(65,72,84),outline=(130,142,160))
    d.line((knob[0],knob[1]-r*.9,knob[0],knob[1]-r*.4),fill=(220,230,240),width=1)
    return im

def palette(images):
    thumb=Image.new('RGB',(256,256*len(images)))
    for i,im in enumerate(images):thumb.paste(im.resize((256,256)),(0,i*256))
    return thumb.quantize(colors=256)

def main():
    root=Path(sys.argv[1]);out=root/'renders';out.mkdir(exist_ok=True)
    ids=json.loads((root/'index.json').read_text())
    studies=[json.loads((root/f'{id}.json').read_text(encoding='utf-8')) for id in ids]
    metrics=[]
    for s in studies:
        a=np.array(s['frames'],dtype=np.float32).reshape(-1,82,3)/255
        light=a.max(axis=2);delta=np.abs(np.diff(a,axis=0)).max(axis=2)
        metrics.append(dict(id=s['look']['id'],lit_fraction=float((light>.20).mean()),mean_level=float(light.mean()),abrupt_fraction=float((delta>.35).mean()),movement=float(delta.mean())))
        frames=[tile(s,i) for i in range(len(s['frames']))]
        pal=palette(frames[::15]);indexed=[im.quantize(palette=pal,dither=Image.Dither.NONE) for im in frames]
        indexed[0].save(out/f"{s['look']['id']}.gif",save_all=True,append_images=indexed[1:],duration=[70,70,60]*60,loop=0,optimize=False)
        # Four moments expose direction, distribution and retained dark space.
        strip=Image.new('RGB',(720,370))
        for j,index in enumerate([0,45,90,135]):strip.paste(frames[index],((j%2)*360,(j//2)*185))
        strip.save(out/f"{s['look']['id']}-study.png")
    for moment,frame in enumerate([0,45,90,135]):
        sheet=Image.new('RGB',(6*300,5*155),(18,20,25))
        for i,s in enumerate(studies):sheet.paste(tile(s,frame,300,155),((i%6)*300,(i//6)*155))
        sheet.save(out/f'contact-{moment}.png')
    (root/'metrics.json').write_text(json.dumps(metrics,indent=2)+'\n')
    selected=json.loads((ROOT/'assets/lighting-collection.json').read_text(encoding='utf-8'))
    by_id={s['look']['id']:s for s in studies}
    collection=root.parent/'lighting-collection';collection.mkdir(exist_ok=True)
    for group in range(3):
        items=[by_id[p['id']] for p in selected[group*6:group*6+6]]
        frames=[]
        for frame in range(180):
            im=Image.new('RGB',(1080,370),(18,20,25))
            for index,s in enumerate(items):im.paste(tile(s,frame),((index%3)*360,(index//3)*185))
            frames.append(im)
        pal=palette(frames[::15]);indexed=[im.quantize(palette=pal,dither=Image.Dither.NONE) for im in frames]
        indexed[0].save(collection/f'collection-{group+1}.gif',save_all=True,append_images=indexed[1:],duration=[70,70,60]*60,loop=0,optimize=False)
        frames[45].save(collection/f'collection-{group+1}.png')
    print(f'{len(studies)} animated studies and four contact sheets rendered.')
if __name__=='__main__':main()

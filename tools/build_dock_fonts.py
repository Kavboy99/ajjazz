"""Bake licensed Inter glyphs to bounded 2-bit coverage; no device access."""
from pathlib import Path
import hashlib,json,math
from PIL import Image,ImageDraw,ImageFont
ROOT=Path(__file__).resolve().parents[1]
SOURCE=ROOT/'research/vendor/inter/InterVariable.ttf'
SHA='4989b125924991b90d05b2d16e0e388c48f7d5bb8b30539bbf9c755278d0ccaf'
def build():
    if hashlib.sha256(SOURCE.read_bytes()).hexdigest()!=SHA:raise ValueError('Inter source changed')
    fonts=[]
    for size,chars,narrow in [(14,'0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ:./%+- ',False),(27,'0123456789:.-',False),(42,'0123456789:.-',False),(10,'0123456789',False),(27,'0123456789:.-',True)]:
        font=ImageFont.truetype(str(SOURCE),size);font.set_variation_by_axes([14,600])
        boxes=[font.getbbox(ch,anchor='ls') for ch in chars]
        top=min(b[1] for b in boxes);bottom=max(b[3] for b in boxes);height=bottom-top
        digits=math.ceil(max(font.getlength(c) for c in '0123456789'))
        glyphs=[];data=[]
        for ch,box in zip(chars,boxes):
            advance=digits if ch.isdigit() else math.ceil(font.getlength(ch))
            width=max(advance,box[2],1);im=Image.new('L',(width,height))
            ImageDraw.Draw(im).text((0,-top),ch,font=font,fill=255,anchor='ls')
            if narrow:
                width=max(1,round(width*0.83));advance=max(1,round(advance*0.83))
                im=im.resize((width,height),Image.Resampling.LANCZOS)
            coverage=[min(3,(v*3+127)//255) for v in im.get_flattened_data()]
            packed=[]
            for i in range(0,len(coverage),4):
                packed.append(sum(v<<(6-j*2) for j,v in enumerate(coverage[i:i+4])))
            glyphs.append(dict(offset=len(data),width=width,advance=advance));data+=packed
        if size==10 and digits*2>14:raise ValueError('Two calendar digits must fit one cell')
        fonts.append(dict(chars=chars,height=height,glyphs=glyphs,data=data))
    out=ROOT/'firmware/aj159-screen/modern-font.json'
    out.write_text(json.dumps({'source_sha256':SHA,'license':'Inter-OFL.txt','fonts':fonts},separators=(',',':'))+'\n')
    print([(f['height'],len(f['data'])) for f in fonts])
if __name__=='__main__':build()

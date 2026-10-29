"""CPU rasterizer for exported egui meshes. No GUI, input, HID or network."""
from pathlib import Path
import json,sys
import numpy as np
from PIL import Image

def render(path):
    data=json.loads(path.read_text())
    assert data['hardware_access'] is False and data['native_windows'] is False
    scale=data['scale'];w,h=round(data['width']*scale),round(data['height']*scale)
    canvas=np.zeros((h,w,4),dtype=np.float32);canvas[:]=[18,20,25,255]
    textures={int(p.stem.split('-')[-1]):np.asarray(Image.open(p).convert('RGBA'),dtype=np.float32)
              for p in path.parent.glob(path.stem+'-texture-*.png')}
    for mesh in data['meshes']:
        tex=textures[mesh['texture']];th,tw=tex.shape[:2]
        nearest=data['nearest'][str(mesh['texture'])]
        xy=np.array([v[:2] for v in mesh['vertices']],dtype=np.float32)*scale
        uv=np.array([v[2:4] for v in mesh['vertices']],dtype=np.float32)
        colors=np.array([v[4] for v in mesh['vertices']],dtype=np.float32)
        clip=np.array(mesh['clip'])*scale
        for indices in np.array(mesh['indices']).reshape(-1,3):
            points=xy[indices];d=points[1]-points[0];e=points[2]-points[0];area=(d[0]*e[1]-d[1]*e[0]).item()
            if abs(area)<1e-6:continue
            if area<0:indices=indices[[0,2,1]];points=xy[indices];area=-area
            x0=max(0,int(np.floor(max(points[:,0].min(),clip[0]))));x1=min(w,int(np.ceil(min(points[:,0].max(),clip[2]))))
            y0=max(0,int(np.floor(max(points[:,1].min(),clip[1]))));y1=min(h,int(np.ceil(min(points[:,1].max(),clip[3]))))
            if x1<=x0 or y1<=y0:continue
            yy,xx=np.mgrid[y0:y1,x0:x1];xx=xx.astype(np.float32)+.5;yy=yy.astype(np.float32)+.5
            weights=[];mask=np.ones(xx.shape,dtype=bool)
            for a,b in [(1,2),(2,0),(0,1)]:
                dx,dy=points[b]-points[a]
                edge=dx*(yy-points[a,1])-dy*(xx-points[a,0])
                inclusive=dy<0 or (dy==0 and dx>0)
                mask &= (edge>=-1e-4) if inclusive else (edge>1e-4)
                weights.append(edge/area)
            if not mask.any():continue
            q=np.stack(weights,axis=-1)[mask];coordinates=q@uv[indices]
            if nearest:
                tx=np.clip((coordinates[:,0]*tw).astype(int),0,tw-1);ty=np.clip((coordinates[:,1]*th).astype(int),0,th-1)
                sampled=tex[ty,tx]
            else:
                tx=np.clip(coordinates[:,0]*tw-.5,0,tw-1);ty=np.clip(coordinates[:,1]*th-.5,0,th-1)
                ix=tx.astype(int);iy=ty.astype(int);jx=np.minimum(ix+1,tw-1);jy=np.minimum(iy+1,th-1)
                fx=(tx-ix)[:,None];fy=(ty-iy)[:,None]
                sampled=(tex[iy,ix]*(1-fx)+tex[iy,jx]*fx)*(1-fy)+(tex[jy,ix]*(1-fx)+tex[jy,jx]*fx)*fy
            src=(q@colors[indices])*sampled/255.0
            dst=canvas[y0:y1,x0:x1][mask]
            canvas[y0:y1,x0:x1][mask]=src+dst*(1-src[:,3:4]/255.0)
    image=Image.fromarray(np.clip(canvas+.5,0,255).astype('uint8')).convert('RGB')
    if scale!=1:image=image.resize((data['width'],data['height']),Image.Resampling.LANCZOS)
    image.save(path.with_suffix('.png'));print(path.with_suffix('.png'))

if __name__=='__main__':
    root=Path(sys.argv[1]);paths=sorted(root.glob('*.json'))
    for path in paths:render(path)

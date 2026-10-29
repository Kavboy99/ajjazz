"""Author bounded AJKS scenes; generated assets never contain device commands."""
from pathlib import Path
import json

ROOT=Path(__file__).resolve().parents[1]
ROWS=json.loads((ROOT/'assets/ak820max-layout.json').read_text(encoding='utf-8'))
KEYS=[(k,r,c) for r,row in enumerate(ROWS) for c,k in enumerate(row)]
def mask(predicate):return sorted(k['id'] for k,r,c in KEYS if predicate(k,r,c))
ODD=mask(lambda k,r,c:(k['id']*17+3)%7<3)
EVEN=mask(lambda k,r,c:k['id'] not in ODD)
TOP=mask(lambda k,r,c:r<3)
BOTTOM=mask(lambda k,r,c:r>=3)
SPARSE=mask(lambda k,r,c:(k['id']*37+11)%11<3)
EDGE=mask(lambda k,r,c:r in (0,5) or c==0 or c==len(ROWS[r])-1)
LEFT=mask(lambda k,r,c:c<7)
RIGHT=mask(lambda k,r,c:c>=7)
WASD=mask(lambda k,r,c:k['physical'] in ['KeyW','KeyA','KeyS','KeyD'])

def layer(effect,a,b=None,speed=.3,scale=1,angle=0,phase=0,opacity=1,keys=None,blend='Add'):
    return dict(name=effect,enabled=True,effect=effect,blend=blend,keys=keys or [],colors=[a,b or a],opacity=opacity,speed=speed,scale=scale,angle=angle,phase=phase)
def base(color=(3,6,14)):return layer('Solid',color,blend='Replace',speed=0)
def veil(speed=.11,scale=1.4,angle=30):return layer('Wave',[48,48,48],[255,255,255],speed,scale,angle,blend='Multiply',opacity=.65)
def scene(name,*layers):return dict(schema=1,name=name,duration=12,fps=15,brightness=.92,layers=list(layers),dock_layers=[])
def look(id,name,family,description,*layers):return dict(id=id,name=name,family=family,description=description,scene=scene(name,*layers))

ALL=[
look('fireflies','Светлячки','nature','Любимый исходный рисунок: мягкие зелёные огоньки на тёмном фоне.',base(),layer('Fireflies',[0,12,3],[170,255,45],.48)),
look('living-forest','Живой лес','nature','Два ритма огоньков и медленно движущаяся тень.',base(),layer('Fireflies',[0,8,1],[100,240,35],.27,keys=ODD),layer('Fireflies',[0,8,5],[20,180,110],.43,phase=.37,keys=EVEN),veil(.075,1.1,65)),
look('biolume','Люмина','nature','Бирюзовые огоньки вспыхивают под мягкой подводной волной.',base(),layer('Fireflies',[0,6,14],[20,225,190],.35),veil(.13,1.7,85)),
look('pollen','Пыльца','nature','Тёплые частицы: медленное золотое мерцание с редкими искрами.',base(),layer('Fireflies',[12,3,0],[255,155,28],.24,keys=EVEN),layer('Fireflies',[4,2,0],[245,220,95],.53,phase=.2,keys=ODD),veil(.09,1,20)),
look('night-garden','Ночной сад','nature','Локальные фиолетовые и зелёные огоньки, разделённые по клавишам.',base(),layer('Fireflies',[0,5,0],[75,200,60],.31,keys=EVEN),layer('Fireflies',[5,0,12],[160,70,215],.2,phase=.55,keys=ODD)),
look('constellation','Созвездия','space','Редкие холодные звёзды над едва заметной туманностью.',base(),layer('Nebula',[3,6,20],[25,30,65],.1,1.3,opacity=.45),layer('Fireflies',[2,5,12],[155,195,255],.32,keys=SPARSE),layer('Fireflies',[0,2,7],[65,120,230],.19,phase=.4,keys=EVEN)),
look('starwind','Звёздный ветер','space','Звёздное поле и тонкие кометные следы на части клавиш.',base(),layer('Fireflies',[0,3,10],[85,135,220],.26),layer('Comet',[0,0,0],[175,220,255],.23,.7,17,opacity=.7,keys=ODD)),
look('nebula-drift','Тихий космос','space','Два разнонаправленных облака с очень медленными переливами.',base(),layer('Nebula',[6,12,65],[115,20,150],.14,1.8,18),layer('Nebula',[0,10,25],[15,135,150],.19,1.2,145,phase=.4,opacity=.48),veil(.07,.8,90)),
look('meteor-shower','Метеоры','motion','Три потока комет движутся под разными углами.',base(),layer('Comet',[0,0,0],[70,185,255],.57,1.4,0,keys=TOP),layer('Comet',[0,0,0],[180,120,255],.41,1.1,180,phase=.4,keys=BOTTOM),layer('Comet',[0,0,0],[230,235,255],.76,1.8,12,opacity=.7,keys=SPARSE)),
look('comet-trail','Орбитальный след','space','Два встречных хвоста на тёмном фоне.',base(),layer('Comet',[0,0,0],[25,220,180],.33,1.1,35),layer('Comet',[0,0,0],[200,115,255],.24,.8,215,phase=.6,opacity=.6)),
look('moon-pool','Лунная рябь','nature','Два независимых круга расходятся по спокойной синей воде.',base([2,7,20]),layer('Raindrops',[0,0,8],[70,170,230],.46,1.1),layer('Raindrops',[0,0,2],[130,205,245],.31,.8,phase=.43,opacity=.7)),
look('deep-tide','Прилив','nature','Волны разной ширины медленно проходят через глубину.',base(),layer('TwinWave',[2,12,35],[10,115,150],.15,1.4,78),layer('Wave',[0,10,30],[20,160,205],.23,.65,95,opacity=.45),veil(.09,.8,10)),
look('ocean-floor','Глубина','nature','Бирюзовые блики на тёмном морском дне.',base(),layer('Nebula',[0,12,35],[5,130,145],.19,2,90),layer('Fireflies',[0,3,7],[45,195,170],.31,opacity=.6),veil(.12,1.3,25)),
look('rain-window','Дождь за окном','nature','Тихие синие струи и отдельные светлые капли.',base(),layer('DigitalRain',[0,0,0],[35,100,170],.25,1.3,opacity=.7),layer('Fireflies',[0,0,2],[100,165,230],.4,keys=SPARSE)),
look('neon-rain','Неоновый дождь','motion','Зелёные потоки с фиолетовыми отражениями.',base(),layer('DigitalRain',[0,0,0],[30,220,130],.55,1.3,keys=EVEN),layer('DigitalRain',[0,0,0],[170,55,230],.37,.9,phase=.35,keys=ODD),layer('Gradient',[0,5,18],[20,5,40],0,.7,90,opacity=.5)),
look('silk','Шёлк','ambient','Два широких световых полотна плавно пересекаются.',base(),layer('TwinWave',[8,15,55],[40,145,175],.15,.85,25),layer('Nebula',[10,2,35],[145,50,170],.12,1.25,135,opacity=.6),veil(.1,1,70)),
look('watercolor','Акварель','ambient','Полупрозрачные цветовые пятна медленно смешиваются.',base(),layer('Mosaic',[20,30,135],[180,40,110],.32,.5,opacity=.75),layer('Mosaic',[0,75,100],[25,185,135],.21,.9,phase=.4,opacity=.5),layer('Nebula',[25,15,55],[90,75,130],.13,1.4,opacity=.3)),
look('northern-lights','Северный свет','ambient','Зелёные занавесы с тонкими фиолетовыми переливами.',base(),layer('Aurora',[0,22,50],[20,160,100],.19,1.3,15),layer('Aurora',[20,0,35],[100,35,155],.13,1.7,160,phase=.4,opacity=.52),veil(.06,1,70)),
look('cherry-smoke','Вишнёвый дым','ambient','Бордовые и лиловые облака с мягкой пространственной тенью.',base(),layer('Nebula',[20,0,15],[170,28,80],.17,1.7,20),layer('Nebula',[5,0,25],[65,25,135],.12,1.1,145,phase=.4,opacity=.6),veil(.08,1.4,90)),
look('copper-embers','Медные угли','nature','Светящиеся угольки без резкого случайного мигания.',base([8,2,0]),layer('Fireflies',[15,0,0],[230,65,8],.3),layer('Fireflies',[6,0,0],[255,155,35],.19,phase=.4,keys=SPARSE),veil(.08,1.2,90)),
look('lava-flow','Лавовый поток','motion','Огненная масса движется волнами под тёмной коркой.',base([10,0,0]),layer('Nebula',[35,0,0],[245,55,5],.28,2,90),layer('Mosaic',[65,65,65],[255,255,255],.35,1.6,blend='Multiply',opacity=.8)),
look('sunset','Закат','ambient','Тёплая нижняя часть и медленный лиловый свет сверху.',base(),layer('Gradient',[155,35,20],[35,15,90],0,.85,90),layer('Wave',[0,0,0],[90,25,80],.12,.8,20,opacity=.35),veil(.09,1.3,130)),
look('dawn','Рассвет','ambient','Спокойное переходящее свечение персиковых и прохладных тонов.',base(),layer('TwinWave',[45,25,65],[180,85,45],.1,.7,65),layer('Nebula',[0,25,50],[35,90,135],.15,1.4,160,opacity=.4)),
look('magnetic','Магнитное поле','motion','Орбитальные полосы пересекает узкий световой луч.',base(),layer('Orbit',[0,15,30],[20,160,160],.31,1.5),layer('Visor',[0,0,0],[155,240,225],.28,1,opacity=.65),veil(.13,2.2,65)),
look('neon-scan','Неоновый след','motion','Бирюзовый и розовый лучи расходятся по разным зонам.',base(),layer('Visor',[0,0,0],[25,220,230],.43,1,keys=LEFT),layer('Visor',[0,0,0],[235,50,140],.29,1,phase=.5,keys=RIGHT),layer('Fireflies',[0,0,0],[45,70,120],.18,opacity=.5)),
look('prism-edge','Призма','motion','Перекрёстные цветовые волны и выделенный контур клавиатуры.',base(),layer('TwinWave',[15,50,100],[100,30,140],.35,1.2,35),layer('Wave',[0,0,0],[70,190,160],.27,1.5,140,keys=EDGE)),
look('echo','Эхо','reactive','Нажатия загораются на фоне едва заметных холодных огоньков.',base(),layer('Fireflies',[0,1,5],[20,55,85],.2,opacity=.55),layer('Reactive',[0,0,0],[45,220,165],.5,keys=EVEN),layer('Reactive',[0,0,0],[155,100,255],.5,keys=ODD)),
look('sonar','Сонар','reactive','Круг от нажатой клавиши расходится по тёмной воде.',base(),layer('Nebula',[0,5,12],[0,25,35],.1,1.2,opacity=.6),layer('Ripple',[0,0,0],[25,220,175],.4,1.15),layer('Reactive',[0,0,0],[150,235,215],.3)),
look('ember-keys','Искры касания','reactive','Тёплый след на каждой нажатой клавише и живой фон.',base(),layer('Fireflies',[0,0,0],[80,20,5],.18),layer('Reactive',[0,0,0],[255,115,30],.4),layer('Ripple',[0,0,0],[145,35,12],.5,1.7,opacity=.3)),
look('city-night','Ночной город','motion','Холодные окна, редкие тёплые огни и горизонтальный свет.',base(),layer('Fireflies',[0,0,5],[25,70,160],.26),layer('Fireflies',[0,0,0],[220,120,45],.2,phase=.4,keys=SPARSE),layer('Comet',[0,0,0],[50,130,190],.28,1.2,0,keys=BOTTOM,opacity=.6)),
]

# Final shortlist; editorial ratings and reasons are in selection.json / LIGHTING-SELECTION.md.
SELECTED=['living-forest','biolume','constellation','copper-embers','pollen','night-garden',
          'meteor-shower','moon-pool','starwind','city-night','comet-trail','neon-rain',
          'sonar','echo','silk','northern-lights','cherry-smoke','magnetic']

if __name__=='__main__':
    out=ROOT/'research/lighting-collection';out.mkdir(parents=True,exist_ok=True)
    (out/'candidates.json').write_text(json.dumps(ALL,ensure_ascii=False,indent=2)+'\n',encoding='utf-8')
    selected=[next(p for p in ALL if p['id']==id) for id in SELECTED]
    (ROOT/'assets/lighting-collection.json').write_text(json.dumps(selected,ensure_ascii=False,indent=2)+'\n',encoding='utf-8')
    examples=ROOT/'examples/collection';examples.mkdir(exist_ok=True)
    for look in selected:(examples/f"{look['id']}.ajazz.json").write_text(json.dumps(look['scene'],ensure_ascii=False,indent=2)+'\n',encoding='utf-8')
    print(f'{len(ALL)} motion studies; {len(selected)} curated looks')

"""Bridges, rival/ork strongholds, camps, ruins and campaign landmarks."""
import copy
import math
import random
from envkit import Model
from architecture import house, keep, masonry_wall, roof_gable, tower
from props import cart, fence, shrine, hay_bale, barrel, sign, brazier
from nature import twig_tree, stump, rock_landmark, rock, mushroom, clump

def clone(source,name,description=None):
    m=copy.deepcopy(source)
    m.name=name
    if description: m.description=description
    return m

def append(target,source,offset=(0,0,0),scale=(1,1,1),angle=0):
    ca,sa=math.cos(angle),math.sin(angle)
    n=len(target.vertices)
    for x,y,z in source.vertices:
        xx,yy=x*scale[0],y*scale[1]
        target.vertices.append((offset[0]+xx*ca-yy*sa,
                                offset[1]+xx*sa+yy*ca,
                                offset[2]+z*scale[2]))
    target.uvs.extend(source.uvs);target.colors.extend(source.colors)
    target.faces.extend(tuple(n+i for i in face) for face in source.faces)

def bridge(name,length=6,width=2.4,stone=False):
    m=Model(name,description="Hand-laid bridge with separated planks, piers and parapets")
    if stone:
        m.box((0,0,.025),(length,width,.05),"cobble")
        for x in (-length/2+.5,0,length/2-.5):
            m.box((x,0,.025),(.35,width+.15,.05),"fieldstone")
        for y in (-width/2,width/2):
            m.box((0,y,.32),(length,.21,.55),"fieldstone")
            for x in range(-2,3):
                m.box((x*1.1,y,.63),(.18,.28,.20),"limestone")
    else:
        for i in range(int(length/.28)):
            x=-length/2+(i+.5)*length/int(length/.28)
            m.box((x,0,.025),(length/int(length/.28)*.91,width,.05),
                  "plank",(.88+.05*math.sin(i*1.83),)*3+(1,))
        for y in (-width*.38,width*.38):
            m.beam((-length/2,y,.04),(length/2,y,.04),.08,.07,"beam")
        for y in (-width/2,width/2):
            for i in range(5):
                x=-length/2+i*length/4
                m.box((x,y,.38),(.11,.11,.76),"beam")
            m.beam((-length/2,y,.71),(length/2,y,.71),.085,.085,"beam")
    return m

def wayside_models():
    out=[bridge("bridge_wood",6,2.4),bridge("bridge_boardwalk",8,1.8),
         bridge("bridge_stone",6,2.4,True),
         clone(shrine(),"wayside_shrine"),
         fence("wayside_fence",5.5,False),
         clone(sign("signpost",1.32),"signpost")]
    cairn=Model("wayside_cairn",description="Stacked waymarker stones")
    for i,(x,y,r) in enumerate(((0,0,.31),(.07,-.02,.24),(-.04,.04,.16))):
        cairn.boulder((x,y,0 if i==0 else .24+.22*(i-1)),r,"granite",501+i)
    out.append(cairn)
    return out

def rival_models():
    out=[]
    k=Model("rival_keep",description="Three-tier sandstone rival keep with battlements")
    for j,(w,h) in enumerate(((6.0,1.10),(4.45,1.05),(3.15,.96))):
        z=sum((1.10,1.05,.96)[:j])
        k.box((0,0,z+h/2),(w,w*.82,h),"limestone",
              (.89-.035*j,.79-.025*j,.65-.02*j,1))
        for sx in (-1,1):
            for sy in (-1,1):
                k.box((sx*w*.44,sy*w*.36,z+h*.5),(.18,.18,h),"fieldstone")
        k.box((0,0,z+h-.02),(w+.16,w*.82+.16,.13),"fieldstone")
    k.box((0,-2.48,.80),(1.10,.05,1.42),"beam")
    for i in range(8):
        x=-1.45+i*.41
        if i%2: k.box((x,-1.5,3.18),(.30,.39,.30),"limestone")
    out.append(k)
    ring=Model("rival_wall_ring",description="Square 24-unit rival bailey with southern sally gap")
    half=12
    for y in (-half,half):
        if y<0:
            ring.box((0,y,.75),(24,.7,1.5),"limestone",(.79,.70,.58,1))
        else:
            for sx in (-1,1):
                ring.box((sx*7.75,y,.75),(8.5,.7,1.5),"limestone",(.79,.70,.58,1))
        for x in range(-11,12,2):
            if y>0 and abs(x)<4: continue
            ring.box((x,y,1.61),(.80,.79,.28),"fieldstone")
    for x in (-half,half):
        ring.box((x,0,.75),(.7,24,1.5),"limestone",(.79,.70,.58,1))
        for y in range(-11,12,2):
            ring.box((x,y,1.61),(.79,.80,.28),"fieldstone")
    for x in (-half,half):
        for y in (-half,half):
            ring.box((x,y,.96),(1.6,1.6,1.92),"fieldstone")
            ring.box((x,y,2.00),(1.88,1.88,.20),"limestone")
    for x in (-3.5,3.5):
        ring.box((x,half,1.05),(.55,.88,2.10),"fieldstone")
    out.append(ring)
    out.append(house("rival_house",2.6,2.1,1.42,"clay_roof",
                     wall="limestone",half_timber=False,chimney=False))
    return out

def ruined(name,w=2.6,d=2.1,stone="fieldstone",seed=0):
    rng=random.Random(seed)
    m=Model(name,description="Collapsed historic masonry with open interior and weathered rubble")
    m.box((0,0,.10),(w,d,.20),stone)
    # Broken walls deliberately have gaps and different surviving heights.
    for side in (-1,1):
        for i in range(5):
            x=-w/2+(i+.5)*w/5
            if (i+seed)%4==0: continue
            h=rng.uniform(.48,1.65)
            m.box((x,side*d/2,h/2),(w/5*.92,.19,h),stone)
    for side in (-1,1):
        for i in range(4):
            y=-d/2+(i+.5)*d/4
            if (i+seed)%3==0: continue
            h=rng.uniform(.45,1.28)
            m.box((side*w/2,y,h/2),(.19,d/4*.9,h),stone)
    for i in range(11):
        a=rng.random()*math.tau;r=rng.uniform(.5,max(w,d)*.7)
        m.boulder((r*math.cos(a),r*math.sin(a),0),rng.uniform(.11,.25),stone,seed+i)
    return m

def old_mill_landmark():
    """Smock mill in the native landmark frame; the +Z Bevy vane mounts at (0,4.75,1.58)."""
    m=Model("ruin_old_mill",description="Full-height tapered boarded mill, thatched cap and working-yard dressing")
    m.cylinder((0,0,.30),1.75,.60,"fieldstone",10,top_radius=1.50)
    m.cylinder((0,0,2.40),1.42,3.60,"plank",8,top_radius=.95,
               colors=(.71,.64,.55,1))
    for z,r in ((1.55,1.32),(2.55,1.20),(3.55,1.07)):
        m.cylinder((0,0,z),r+.045,.12,"beam",8,top_radius=r-.02)
    # Front is Bevy +Z, which is Blender -Y. The sail hub remains in front of this cap.
    m.box((0,-1.53,.85),(.62,.075,1.15),"iron",(.40,.38,.34,1))
    for x in (-.42,.42): m.box((x,-1.55,.88),(.14,.20,1.25),"beam")
    m.box((0,-1.55,1.50),(.95,.24,.16),"beam")
    m.box((0,-1.84,.07),(.85,.55,.14),"fieldstone")
    m.box((-1.22,-.30,2.50),(.07,.34,.40),"iron",(.31,.32,.31,1))
    # Ghost window attachment centre (Bevy x=.57,y=3.1,z=.88).
    m.box((.57,-.92,3.10),(.28,.07,.37),"iron",(.55,.57,.48,1))
    for x in (.39,.75): m.box((x,-.99,3.10),(.065,.10,.45),"beam")
    m.cylinder((0,0,4.245),1.24,.07,"beam",8,top_radius=1.10)
    m.cylinder((0,0,4.48),1.22,.55,"thatch",8,top_radius=.78)
    m.cylinder((0,0,5.00),.78,.50,"thatch",8,top_radius=.30)
    m.cylinder((0,0,5.46),.32,.42,"thatch",8,top_radius=.02)
    m.cylinder((0,0,5.63),.09,.12,"beam",8)
    # Ivy in broad patches, and a few recognisable mill-yard remnants.
    for x,y,z,r in ((-1.04,.78,.52,.22),(-1.15,.55,1.14,.18),(-.96,.75,2.12,.15)):
        m.boulder((x,y,z),r,"wattle",int(z*61),(.53,.72,.43,1))
    m.cylinder((1.62,-.95,.54),.48,.18,"fieldstone",12)
    for x,y in ((.95,-1.7),(1.30,-1.45),(1.05,-1.35)):
        m.box((x,y,.17),(.36,.29,.34),"straw")
    for x,y in ((0,-2.5),(.35,-3.1),(-.25,-3.65)):
        m.box((x,y,.05),(.49,.39,.10),"fieldstone")
    return m

def witch_hut_landmark():
    m=Model("ruin_witch_hut",description="Crooked stilt witch hut, roof, chimney and cauldron at native animation anchors")
    for x,y in ((-1,.85),(.95,.90),(-1,-.8),(1,-.85),(0,.95),(.05,-.90)):
        m.beam((x*1.07,y*1.12,0),(x,y,1.40),.13,.12,"beam")
    m.box((0,0,1.33),(2.6,2.2,.13),"plank")
    for i in range(5): m.box((0,-.87+i*.43,1.42),(2.50,.34,.035),"plank")
    # Main cabin Bevy depth -0.1: front +Z therefore Blender Y=-0.7.
    m.box((-.05,.10,2.15),(1.9,1.6,1.5),"wattle",(.74,.70,.62,1))
    for x in (-1.0,.90):
        for y in (-.68,.88): m.box((x,y,2.15),(.14,.14,1.56),"beam")
    for z in (1.63,2.04,2.45):
        for y in (-.70,.90): m.box((-.05,y,z),(1.94,.07,.045),"beam")
    m.box((.32,-.75,1.95),(.50,.06,.95),"iron",(.32,.27,.23,1))
    m.box((.32,-.80,2.46),(.60,.13,.10),"beam")
    # Windows at the precise locations of the two independently glowing children.
    m.box((-.55,-.72,2.30),(.33,.055,.33),"iron",(.22,.27,.20,1))
    m.box((-1.065,.25,2.20),(.06,.32,.32),"iron",(.22,.27,.20,1))
    for x in (-.75,-.36): m.box((x,-.79,2.30),(.045,.09,.40),"beam")
    for y in (.05,.44): m.box((-1.11,y,2.20),(.09,.045,.40),"beam")
    # Fully solid gable roof, with eave under the 3.66 ridge.
    roof_gable(m,2.35,2.45,2.91,.74,"thatch",over=.08)
    m.box((-.78,.72,3.37),(.42,.43,.85),"fieldstone")
    m.box((-.78,.72,3.80),(.52,.53,.10),"fieldstone")
    # Front ladder and native cauldron at Bevy (1.75,0,1.3).
    for x in (-.28,.28): m.beam((x,-1.60,0),(x,-1.14,1.36),.07,.06,"beam")
    for z in (.34,.68,1.02): m.beam((-.33,-1.55+z*.30,z),(.33,-1.55+z*.30,z),.055,.05,"beam")
    m.cylinder((1.75,-1.30,.49),.41,.70,"iron",10,top_radius=.44)
    m.cylinder((1.75,-1.30,.86),.46,.09,"iron",10)
    for i in range(6):
        a=i*math.tau/6
        m.boulder((1.75+.52*math.cos(a),-1.30+.52*math.sin(a),.01),.13,"fieldstone",40+i)
    return m

def frozen_spire_landmark():
    m=Model("ruin_frozen_spire",description="Wind-carved crystalline blade erupting six units above a frosted tor")
    def shard(x,y,height,radius,lean_x,lean_y,cell,color,sides=5):
        rings=[]
        for zz,rr,shift in ((0,.78,0),(.34,1,0),(height*.76,.47,.69),(height,.04,1)):
            rings.append([(x+radius*rr*math.cos(i*math.tau/sides)+lean_x*shift,
                           y+radius*rr*math.sin(i*math.tau/sides)+lean_y*shift,zz)
                          for i in range(sides)])
        for low,high in zip(rings,rings[1:]):
            for i in range(sides): m.quad(low[i],low[(i+1)%sides],high[(i+1)%sides],high[i],cell,color)
    m.boulder((0,0,.02),.75,"granite",22,(.70,.78,.85,1))
    shard(0,0,6.55,.64,-.18,.12,"plaster",(.50,.70,.86,1),6)
    shard(-.26,.15,5.8,.34,.05,.18,"plaster",(.74,.88,.95,1))
    shard(.65,.20,4.72,.30,.20,.05,"slate",(.49,.68,.88,1))
    shard(-.72,-.42,3.20,.32,-.22,-.15,"plaster",(.54,.74,.92,1))
    shard(.90,.58,2.60,.29,.27,.09,"slate",(.52,.72,.91,1))
    # The heart's animation is centred at Bevy (.38,2.35,.52): face points Blender -Y.
    m.box((.38,-.45,2.35),(.36,.09,.40),"slate",(.31,.44,.57,1))
    for i in range(8):
        a=i*math.tau/8+.27;r=1.80+.10*(i%3)
        m.boulder((r*math.cos(a),r*math.sin(a),.04),.40+.05*(i%2),"plaster",55+i,
                  (.68,.81,.91,1))
    return m

def sunken_pyramid_landmark():
    m=Model("ruin_sunken_pyramid",description="Six-tier stepped desert shrine with native sun-disc clearance")
    for i in range(6):
        w=2.35+(0.62-2.35)*i/5
        z=i*.6
        m.box((0,0,z+.30),(2*w,2*w,.60),"limestone",(.86-.025*(i%3),.75-.015*i,.57,1))
        if i%2==0: m.box((0,0,z+.615),(2*w+.07,2*w+.07,.06),"fieldstone",(.93,.81,.61,1))
        for sx in (-1,1):
            for sy in (-1,1): m.box((sx*(w-.07),sy*(w-.07),z+.31),(.13,.13,.58),"fieldstone")
    # Stair on +Z Bevy = -Y Blender. Tread fronts track the tier taper.
    for s in range(12):
        z=s*.30;w=2.35+(0.62-2.35)*(z/3.6)
        m.box((0,-w-.14,z+.10),(.95-z*.04,.62,.40),"limestone",(.95,.83,.64,1))
    m.box((0,0,3.70),(1.50,1.50,.20),"fieldstone")
    for x in (-.52,.52):
        for y in (-.52,.52):
            m.cylinder((x,y,4.11),.11,.78,"limestone",6)
            m.box((x,y,4.54),(.30,.30,.10),"fieldstone")
    for y in (-.52,.52): m.box((0,y,4.66),(1.40,.34,.15),"fieldstone")
    m.box((0,0,3.90),(.44,.44,.34),"bronze",(.90,.69,.32,1))
    m.box((0,-.65,3.33),(.50,.12,.42),"iron",(.35,.30,.24,1))
    # Off-axis fallen statuary/half-buried stone gives the monument a historic silhouette.
    for x,y,r in ((-2.4,1.4,1.0),(-1.5,2.4,.83),(-2.8,-.2,.75),(-.2,2.75,.72)):
        m.boulder((x,y,.01),r,"earth",int((x+y)*74),(.87,.76,.56,1))
    m.cylinder((1.15,-3.15,.75),.17,1.45,"fieldstone",4,top_radius=.11)
    m.cylinder((1.15,-3.15,1.62),.12,.30,"bronze",4,top_radius=.01)
    return m

def standing_stones_landmark():
    m=Model("ruin_standing_stones",description="Six worked megaliths on a 2.9-unit ring with walkable inner altar")
    spec=((1.19,3.25),(1.95,3.25),(2.85,2.60),(3.75,2.40),(4.65,2.80),(5.50,2.55))
    for i,(a,h) in enumerate(spec):
        x=2.9*math.cos(a);y=-2.9*math.sin(a)  # negate Bevy Z
        mon=Model("one_stone")
        mon.box((0,0,h*.30),(.74,.48,h*.60),"granite",(.75+.025*(i%3),.77+.018*(i%3),.76,1))
        mon.box((0,.01,h*.72),(.52,.36,h*.48),"granite",(.81,.83,.82,1))
        mon.box((0,.01,h*.99),(.64,.49,.18),"limestone",(.82,.80,.73,1))
        # Tangent local x, inward local y; glowing rune strips sit at inner radius 2.63.
        append(m,mon,(x,y,0),angle=-a+math.pi/2)
        m.boulder((x,y,.01),.30,"fieldstone",300+i)
    a1,a2=spec[0][0],spec[1][0]
    x1,y1=2.9*math.cos(a1),-2.9*math.sin(a1)
    x2,y2=2.9*math.cos(a2),-2.9*math.sin(a2)
    m.beam((x1,y1,3.44),(x2,y2,3.44),.56,.45,"granite")
    m.box((0,0,.52),(1.30,1.0,.18),"granite")
    m.box((0,0,.68),(1.05,.80,.14),"limestone")
    m.cylinder((0,0,.78),.16,.10,"fieldstone",8)
    a=.22;x=3.35*math.cos(a);y=-3.35*math.sin(a)
    m.beam((x-.68,y,.30),(x+.65,y-.24,.30),.48,.46,"granite")
    return m

def ruin_models():
    out=[
        ruined("ruin_wall",2.9,1.8,seed=11),
        ruined("ruin_arch",2.2,1.4,seed=12),
        old_mill_landmark(),
        witch_hut_landmark(),
        frozen_spire_landmark(),
        sunken_pyramid_landmark(),
        standing_stones_landmark(),
        ruined("poi_burned_cabin",2.2,1.9,stone="beam",seed=18),
        ruined("poi_fallen_watchtower",2.8,2.5,seed=19),
        ruined("vignette_fallen_tower",3.1,2.8,seed=20),
        ruined("vignette_sunken_wreck",3.0,1.8,stone="beam",seed=21),
    ]
    column=Model("ruin_column",description="Carved fractured standing column")
    column.box((0,0,.14),(.60,.60,.28),"limestone")
    column.cylinder((0,0,.75),.22,1.05,"limestone",10)
    column.box((0,0,1.27),(.55,.55,.12),"limestone")
    out.append(column)
    graves=Model("poi_graves",description="Three tilted grave markers and stone burial surrounds")
    for x,y,h in ((-.55,0,.47),(0,-.28,.63),(.52,.10,.40)):
        graves.box((x,y,h/2),(.30,.13,h),"granite")
        graves.box((x,y-.07,h*.72),(.12,.02,.03),"limestone")
    out.append(graves)
    stones=Model("poi_standing_stones",description="Ancient uneven standing stone ring")
    for i in range(7):
        a=i*math.tau/7
        stones.boulder((1.1*math.cos(a),1.1*math.sin(a),0),.46,"granite",30+i)
        stones.beam((1.1*math.cos(a),1.1*math.sin(a),.20),
                    (1.15*math.cos(a),1.17*math.sin(a),1.17),
                    .30,.26,"granite")
    out.append(stones)
    gallows=Model("poi_gallows",description="Weathered timber gallows without character content")
    for x in (-.73,.73): gallows.box((x,0,1.1),(.14,.16,2.2),"beam")
    gallows.beam((-.80,0,2.19),(.80,0,2.19),.16,.16,"beam")
    for x in (-.35,.35): gallows.beam((x,0,2.14),(x,0,1.65),.016,.018,"beam")
    out.append(gallows)
    out.append(house("poi_shepherd_hut",2.0,1.7,1.15,"thatch",
                     wall="wattle",half_timber=False))
    field=Model("poi_field",description="Stone-edged rural field with planted crop rows")
    field.box((0,0,.03),(4.4,3.4,.06),"earth")
    for x in (-2.2,2.2): field.box((x,0,.12),(.12,3.5,.18),"fieldstone")
    for y in (-1.7,1.7): field.box((0,y,.12),(4.5,.12,.18),"fieldstone")
    for y in (-1.16,-.55,.06,.67,1.28):
        field.box((0,y,.11),(3.7,.23,.12),"wattle",(.50,.69,.34,1))
    out.append(field)
    bog_tower=ruined("bog_tower_a",2.1,2.1,"fieldstone",seed=58)
    for x in (-.82,.82):
        for y in (-.82,.82):
            bog_tower.box((x,y,.60),(.18,.19,1.20),"beam")
    out.append(bog_tower)
    bog_hut=house("bog_hut_a",2.2,1.8,1.24,"thatch",
                  wall="wattle",half_timber=True)
    bog_hut.vertices=[(x,y,z+.52) for x,y,z in bog_hut.vertices]
    # Visible old piles support the stilt hut without changing its ground origin.
    for x in (-.82,.82):
        for y in (-.64,.64):
            bog_hut.box((x,y,.31),(.11,.11,.62),"beam")
    out.append(bog_hut)
    ob=Model("poi_obelisk",description="Weathered obelisk landmark")
    ob.box((0,0,.14),(.81,.81,.28),"granite")
    ob.box((0,0,1.03),(.54,.54,1.57),"granite")
    ob.cylinder((0,0,1.96),.34,.48,"limestone",4,top_radius=.015)
    out.append(ob)
    out.append(clone(shrine(),"poi_altar"))
    out.append(clone(ruined("vignette_abandoned_camp",2.0,1.5,seed=22),
                     "vignette_abandoned_camp"))
    caravan=Model("vignette_lost_caravan",description="Abandoned cart and supplies")
    append(caravan,cart(),(-.3,0,0))
    append(caravan,barrel(),(.67,.42,0))
    out.append(caravan)
    frozen=Model("vignette_frozen_camp",description="Snowbound campsite and buried hearth")
    append(frozen,brazier(),(0,0,0))
    for x,y in ((-.6,.2),(.5,-.3)):
        frozen.boulder((x,y,0),.34,"plaster",int(x*100+50),(.90,.95,1,1))
    out.append(frozen)
    return out

def ork_gate_door(name,sign):
    m=Model(name,description="Hinged ironbound spiked gate leaf")
    cx=-sign*1.475
    m.box((cx,0,1.5),(2.95,.16,3.0),"beam")
    for i in range(4):
        x=cx-1.18+i*.79
        m.box((x,-.11,1.5),(.085,.06,2.92),"beam")
    for z in (.70,2.20):
        m.box((cx,-.15,z),(2.84,.07,.13),"iron")
    for i in range(5):
        x=cx-1.15+i*.56
        m.cylinder((x,-.19,1.53),.045,.08,"iron",7)
    return m

def ork_models():
    out=[]
    pal=Model("ork_palisade_run",description="Six-unit rough sharpened oak palisade")
    for i in range(11):
        x=-3+i*.6
        h=1.50+.18*math.sin(i*4.1)
        pal.cylinder((x,0,h/2),.19,h,"beam",6,top_radius=.075)
        pal.cylinder((x,0,h+.12),.085,.25,"bone",5,top_radius=.004)
    for z in (.43,1.17): pal.beam((-3.2,.18,z),(3.2,.18,z),.11,.10,"beam")
    out.append(pal)
    gate=Model("ork_gate",description="Massive rough-hewn gate frame with horned lintel")
    for x in (-3.45,3.45):
        gate.box((x,0,1.8),(.55,.75,3.60),"beam")
        gate.box((x,0,3.55),(.69,.87,.42),"bone")
    gate.box((0,0,3.34),(7.15,.73,.54),"beam")
    for x in (-2.3,-.8,.8,2.3):
        gate.cylinder((x,0,3.78),.17,.41,"bone",6,top_radius=.012)
    out.extend((gate,ork_gate_door("ork_gate_door_left",-1),
                ork_gate_door("ork_gate_door_right",1)))
    hall=Model("ork_hall",description="Full-scale great hall, pitched hide roof and native banner/torch anchors")
    hall.box((0,0,.13),(11.4,8.4,.26),"fieldstone")
    for x in (-5.45,5.45): hall.box((x,0,2.42),(.45,7.95,4.57),"beam")
    for y in (-3.88,3.88):
        hall.box((0,y,2.42),(10.9,.46,4.57),"plank",(.74,.65,.55,1))
    for x in (-5.35,-3.20,-1.10,1.10,3.20,5.35):
        for y in (-3.9,3.9): hall.box((x,y,2.45),(.20,.22,4.70),"beam")
    for x in (-5.4,5.4):
        for y in (-3.9,3.9): hall.beam((x,y,.36),(x*.78,y,3.30),.16,.15,"beam")
    # Native hall door and torches are on Bevy -Z, i.e. Blender +Y.
    hall.box((0,4.14,1.83),(1.62,.09,3.15),"iron",(.34,.28,.23,1))
    for x in (-.92,.92): hall.box((x,4.20,1.86),(.16,.20,3.38),"beam")
    hall.box((0,4.18,3.54),(2.18,.21,.22),"bone")
    for x in (-3.0,3.0):
        hall.box((x,4.18,2.80),(.27,.09,.45),"iron")
    roof_gable(hall,11.8,8.75,4.66,1.59,"thatch",over=.12)
    for y in (-3.78,3.78): hall.beam((0,y,6.28),(0,y*.95,6.33),.16,.15,"bone")
    # A fixed rear roof pole passes through the dynamic banner attachment at Bevy +Z3.2.
    hall.cylinder((0,-3.20,7.31),.07,1.72,"beam",8)
    hall.cylinder((0,-3.20,8.19),.15,.14,"bone",6,top_radius=.02)
    for x in (-2.35,2.35):
        hall.cylinder((x,4.15,3.72),.23,.50,"bone",6,top_radius=.02)
    out.append(hall)
    spire=Model("ork_spire",description="Warped dark-timber ritual spire with horn crown")
    spire.box((0,0,.20),(3.4,3.4,.40),"fieldstone")
    z=.40
    for j,(w,h) in enumerate(((2.6,2.4),(2.25,2.3),(1.87,2.2),(1.48,2.1),(1.10,2.0))):
        spire.box((0,0,z+h/2),(w,w,h),"beam",(.54+.025*j,.48+.019*j,.42,1))
        for x in (-w/2,w/2):
            for y in (-w/2,w/2):
                spire.beam((x,y,z+.05),(x*.77,y*.77,z+h-.05),.15,.14,"bone")
        spire.box((0,0,z+h-.08),(w+.22,w+.22,.16),"iron")
        z+=h
    spire.cylinder((0,0,12.08),.35,1.35,"iron",8,top_radius=.07)
    spire.cylinder((.45,.30,11.72),.34,.18,"iron",10)
    spire.cylinder((.45,.30,11.84),.23,.09,"bronze",10)
    spire.cylinder((-.80,0,11.40),.06,2.40,"beam",8)
    spire.cylinder((-.80,0,12.64),.12,.15,"bone",7,top_radius=.01)
    out.append(spire)
    tw=Model("ork_tower",description="Elevated open timber watchtower")
    for x in (-.62,.62):
        for y in (-.62,.62):
            tw.beam((x,y,0),(x*.97,y*.97,6.50),.23,.22,"beam")
    for z in (1.35,2.68,4.28):
        for x in (-.62,.62): tw.beam((x,-.62,z),(x,.62,z),.10,.09,"beam")
    tw.box((0,0,4.55),(2.10,2.10,.20),"plank")
    for x in (-1.0,1.0): tw.box((x,0,5.17),(.14,2.04,.90),"beam")
    for y in (-1.0,1.0): tw.box((0,y,5.17),(2.04,.14,.90),"beam")
    roof_gable(tw,2.22,2.22,5.70,.85,"thatch",over=.10)
    tw.cylinder((0,0,6.02),.055,1.90,"beam",8)
    tw.cylinder((0,0,6.98),.10,.10,"bone",7,top_radius=.01)
    out.append(tw)
    out.append(house("ork_longhouse",4.4,2.8,1.63,"thatch",
                     wall="plank",half_timber=True))
    out.append(house("ork_forge",3.0,2.5,1.75,"slate",
                     wall="fieldstone",half_timber=True,chimney=True))
    pen=fence("ork_pen",4.6)
    append(pen,fence("pen_back",4.6),(0,2.1,0))
    out.append(pen)
    tent=Model("ork_tent",description="Hide tent stretched over forked pole frame")
    tent.box((0,0,.05),(2.15,2.0,.10),"earth")
    tent.quad((0,-1.03,1.64),(0,1.03,1.64),
              (-1.16,1.03,.08),(-1.16,-1.03,.08),
              "burgundy_cloth",(.72,.66,.56,1))
    tent.quad((1.16,-1.03,.08),(1.16,1.03,.08),
              (0,1.03,1.64),(0,-1.03,1.64),
              "burgundy_cloth",(.72,.66,.56,1))
    tent.beam((0,-1.12,1.65),(0,1.12,1.65),.10,.11,"beam")
    out.append(tent)
    drum=Model("ork_drum",description="Skin-faced war drum")
    drum.cylinder((0,0,.39),.39,.78,"beam",11,top_radius=.37)
    for z in (.07,.70): drum.cylinder((0,0,z),.39,.055,"bone",11)
    for i in range(8):
        a=i*math.tau/8
        drum.beam((.38*math.cos(a),.38*math.sin(a),.10),
                  (.38*math.cos(a+.12),.38*math.sin(a+.12),.68),
                  .022,.021,"bone")
    out.append(drum)
    rack=Model("ork_rack",description="Spiked weapon drying rack")
    for x in (-.80,.80): rack.box((x,0,.78),(.13,.14,1.56),"beam")
    for z in (.58,1.31): rack.beam((-.84,0,z),(.84,0,z),.10,.10,"beam")
    for x in (-.50,-.12,.26,.59):
        rack.beam((x,-.05,.42),(x,-.05,1.62),.035,.034,"iron")
    out.append(rack)
    spit=Model("ork_spit",description="Roasting spit above stone-ringed fire")
    for x in (-.73,.73):
        spit.beam((x,0,0),(x,0,.90),.11,.10,"beam")
    spit.beam((-.82,0,.72),(.82,0,.72),.045,.045,"iron")
    for i in range(9):
        a=i*math.tau/9
        spit.boulder((.42*math.cos(a),.37*math.sin(a),0),.12,"fieldstone",i)
    out.append(spit)
    pile=Model("ork_pile",description="Stolen lumber and scrap pile")
    for i in range(6):
        a=i*1.7
        pile.beam((-.75*math.cos(a),-.55*math.sin(a),.12+i*.06),
                  (.75*math.cos(a),.55*math.sin(a),.12+i*.06),
                  .15,.13,"beam")
    out.append(pile)
    bp=Model("ork_banner_pole",description="Blackened banner pole; animated cloth stays native")
    bp.cylinder((0,0,2.20),.075,4.40,"beam",8,top_radius=.044)
    bp.cylinder((0,0,4.50),.13,.20,"bone",6,top_radius=.008)
    out.append(bp)
    cb=Model("camp_banner",description="Three-unit camp pole at native camp-local z=-1.4; cloth remains animated")
    cb.cylinder((0,1.4,1.50),.035,3.0,"beam",8)
    out.append(cb)
    br=Model("ork_torch_bracket",description="Iron torch bracket under dynamic flame")
    br.beam((0,0,0),(0,0,.82),.08,.07,"iron")
    br.cylinder((0,0,.82),.12,.09,"iron",8)
    out.append(br)
    totem=Model("ork_totem",description="Stacked carved totem with horned crown")
    totem.cylinder((0,0,.10),.29,.20,"beam",8)
    for i,w in enumerate((.68,.58,.49,.43)):
        z=.22+i*.54
        totem.box((0,0,z+.24),(w,w*.91,.48),"beam")
        totem.box((0,-w*.48,z+.33),(w*.63,.04,.07),"bone")
        totem.box((0,0,z+.51),(w+.04,w+.04,.06),"burgundy_cloth")
    for x in (-.30,.30):
        totem.beam((x,0,2.38),(x*1.34,0,2.82),.075,.07,"bone")
    out.append(totem)
    spikes=Model("ork_spikes",description="Sharpened warning stakes")
    for i,x in enumerate((-.34,0,.36)):
        h=1.05+i*.19
        spikes.cylinder((x,0,h/2),.08,h,"beam",6,top_radius=.04)
        spikes.cylinder((x,0,h+.11),.045,.23,"bone",5,top_radius=.002)
    out.append(spikes)
    out.append(clone(spikes,"camp_spikes"))
    bon=Model("ork_bonfire_base",description="Charcoal and stacked logs around fire pit")
    for i in range(12):
        a=i*math.tau/12
        bon.boulder((.45*math.cos(a),.45*math.sin(a),0),.13,"fieldstone",i)
    for i in range(4):
        a=i*math.tau/4
        bon.beam((-.45*math.cos(a),-.45*math.sin(a),.14),
                 (.45*math.cos(a),.45*math.sin(a),.14),.17,.14,"beam")
    out.append(bon)
    return out

def camp_models():
    tent=Model("camp_tent",description="Canvas ridge tent over a forked timber frame")
    tent.box((0,0,.035),(2.3,1.8,.07),"earth")
    tent.quad((0,-.87,1.39),(0,.87,1.39),
              (-1.10,.98,.08),(-1.10,-.98,.08),
              "burgundy_cloth",(.74,.69,.60,1))
    tent.quad((1.10,-.98,.08),(1.10,.98,.08),
              (0,.87,1.39),(0,-.87,1.39),
              "burgundy_cloth",(.74,.69,.60,1))
    tent.beam((0,-1.02,1.40),(0,1.02,1.40),.09,.09,"beam")
    for y in (-.85,.85):
        tent.box((0,y,.68),(.08,.09,1.36),"beam")
    out=[tent]
    cage=Model("camp_cage",description="Open three-sided prisoner cage with separate hinged doorway")
    cage.box((0,0,.07),(2.34,2.34,.14),"beam")
    for x in (-1.10,1.10):
        for y in (-1.10,1.10):
            cage.box((x,y,.93),(.12,.12,1.86),"beam")
    for x in (-1.10,1.10):
        cage.beam((x,-1.12,1.78),(x,1.12,1.78),.09,.09,"beam")
    for y in (-1.10,1.10):
        cage.beam((-1.12,y,1.78),(1.12,y,1.78),.09,.09,"beam")
    for y in (-.75,-.35,.05,.45,.85):
        cage.box((-1.1,y,.91),(.055,.055,1.60),"beam")
    for x in (-.75,-.35,.05,.45,.85):
        for y in (-1.1,1.1):
            cage.box((x,y,.91),(.055,.055,1.60),"beam")
    out.append(cage)
    door=Model("camp_cage_door",description="Pivot-local cage grille closing the +X face")
    for y in (.09,.74,1.38,1.97):
        door.box((0,y,.87),(.055,.07,1.65),"beam")
    for z in (.42,1.42):
        door.beam((0,0,z),(0,2.05,z),.08,.08,"beam")
    out.append(door)
    out.append(clone(brazier(),"camp_firepit"))
    return out

def blight_models():
    out=[
        clone(twig_tree("dead",2.4,False,811),"blight_dead_tree"),
        clone(twig_tree("claw",1.8,False,812),"blight_claw_tree"),
        clone(twig_tree("snag",1.35,False,813),"blight_snag"),
        clone(stump(),"blight_stump"),
        clone(mushroom(),"blight_shroom"),
        clone(ork_models_base_spikes(),"blight_spikes"),
        clone(sign("waypost",1.55),"blight_waypost"),
    ]
    for name,seed in (("blight_bone_pile",21),("blight_scrap",23),
                      ("blight_ribcage",25),("blight_impale",27)):
        m=Model(name,description="Original sculpted Blight battlefield dressing")
        rng=random.Random(seed)
        for i in range(7):
            a=i*math.tau/7
            x=.30*math.cos(a);y=.25*math.sin(a)
            m.beam((x,y,0),(x+rng.uniform(-.13,.13),y,.17+rng.uniform(.06,.22)),
                   .045,.04,"bone" if "bone" in name or "rib" in name else "beam")
        if name=="blight_impale":
            m.beam((0,0,0),(0,0,1.57),.13,.13,"beam")
            m.cylinder((0,0,1.62),.11,.16,"bone",7)
        out.append(m)
    for name in ("blight_mud_pool","blight_warp_pool","blight_tar_pit"):
        m=Model(name,description="Irregular shallow Blight pool rim")
        m.box((0,0,.012),(1.30,.93,.024),"mud" if name!="blight_warp_pool" else "fieldstone")
        for i in range(9):
            a=i*math.tau/9
            m.boulder((.64*math.cos(a),.46*math.sin(a),0),.10,"fieldstone",i)
        out.append(m)
    gib=Model("blight_gibbet",description="Blackened timber gibbet")
    gib.beam((0,0,0),(0,0,2.3),.17,.15,"beam")
    gib.beam((0,0,2.18),(.97,0,2.18),.14,.12,"beam")
    gib.beam((.80,0,2.18),(.80,0,1.63),.025,.023,"beam")
    out.append(gib)
    eff=Model("blight_effigy",description="Tattered war effigy on spiked post")
    eff.beam((0,0,0),(0,0,2.07),.13,.11,"beam")
    eff.beam((-.55,0,1.44),(.55,0,1.44),.09,.09,"beam")
    eff.box((0,-.03,1.38),(.48,.07,.64),"burgundy_cloth")
    out.append(eff)
    vent=Model("blight_vent",description="Rock-framed vent; smoke and glow stay native")
    for i in range(8):
        a=i*math.tau/8
        vent.boulder((.37*math.cos(a),.37*math.sin(a),0),.18,"granite",i)
    out.append(vent)
    return out

def ork_models_base_spikes():
    m=Model("temporary_spikes")
    for i,x in enumerate((-.30,0,.32)):
        h=1.12+i*.11
        m.cylinder((x,0,h/2),.075,h,"beam",6,top_radius=.033)
        m.cylinder((x,0,h+.1),.04,.2,"bone",5,top_radius=.002)
    return m

def world_models():
    shop=house("merchant_shop",2.85,2.20,1.54,"clay_roof",
               wall="plaster",half_timber=True,chimney=True)
    shop.box((0,-1.21,.78),(1.28,.28,.13),"plank")
    shop.quad((-.88,-1.46,1.26),(.88,-1.46,1.26),
              (.75,-1.01,1.72),(-.75,-1.01,1.72),"burgundy_cloth")
    orchard=twig_tree("orchard_apple_tree",1.68,True,529)
    for i in range(44):
        a=i*2.39996
        r=.14+.52*math.sqrt((i+.5)/44)
        z=.80+.61*((i*17)%43)/43
        orchard.card((r*math.cos(a),r*math.sin(a),z),.28,.27,
                     "leaf",a,colors=(.73,.91,.51,1))
    overlook=Model("vista_overlook",description="Stone overlook plinth with carved railing and lookout marker")
    overlook.box((0,0,.11),(3.1,1.9,.22),"fieldstone")
    for x in (-1.43,1.43):
        for y in (-.83,.83):
            overlook.box((x,y,.49),(.18,.17,.76),"limestone")
    for y in (-.83,.83):
        overlook.beam((-1.43,y,.82),(1.43,y,.82),.11,.11,"limestone")
    overlook.cylinder((0,.51,.55),.12,.68,"bronze",8)
    cascade=Model("vista_cascade",description="Sculpted rock cascade shell; animated water remains native")
    for x,y,s in ((-.70,0,.73),(.54,.24,.65),(0,-.42,.60),
                  (-.52,.72,.47),(.80,-.50,.44)):
        cascade.boulder((x,y,0),s,"granite",int((x+2)*100))
    cascade.box((0,0,.025),(1.23,1.50,.05),"granite",(.61,.69,.72,1))
    return (wayside_models()+rival_models()+ruin_models()+ork_models()+
            camp_models()+blight_models()+[shop,orchard,overlook,cascade])

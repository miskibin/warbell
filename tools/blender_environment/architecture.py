"""Stone-and-timber medieval campaign architecture, modeled for Bevy scale."""
import math
import random
from envkit import Model, tint

def timber_frame(m,w,d,h,beam=.09,cell="beam"):
    for x in (-w/2,w/2):
        for y in (-d/2,d/2):
            m.box((x,y,h/2),(beam,beam,h),cell)
    for z in (.12,h-.08):
        for y in (-d/2,d/2):
            m.box((0,y,z),(w+beam,beam,beam),cell)
        for x in (-w/2,w/2):
            m.box((x,0,z),(beam,d+beam,beam),cell)
    for x in (-w/2,w/2):
        for y in (-d/2,d/2):
            m.beam((x,y,.16),(x*.8,y,h-.13),beam*.5,beam*.5,cell)

def roof_gable(m,w,d,eave,rise,cell,over=.13):
    x=w/2+over;y=d/2+over
    # Counterclockwise from above.  Blender's preview material is double-sided,
    # but Bevy culls opaque backs; the slope normals must have positive Z.
    m.quad((0,-y,eave+rise),(0,y,eave+rise),(-x,y,eave),
           (-x,-y,eave),cell)
    m.quad((x,-y,eave),(x,y,eave),(0,y,eave+rise),
           (0,-y,eave+rise),cell)
    # Exposed ridge and rafters break the roof's plane silhouette.
    m.beam((0,-y,eave+rise+.025),(0,y,eave+rise+.025),.065,.065,"beam")
    for sy in (-y,y):
        for sx in (-x,x):
            m.beam((0,sy,eave+rise),(sx,sy,eave),.06,.065,"beam")
    # Decorative overlapping shingle edges on eave.
    for sx in (-1,1):
        m.beam((sx*x,-y,eave-.015),(sx*x,y,eave-.015),.045,.045,"beam")

def doorway(m,w,d,h,door_w=.48,door_h=.95,wall="plaster",door="plank"):
    front=-d/2
    side=(w-door_w)/2
    for sign in (-1,1):
        m.box((sign*(door_w/2+side/2),front,h/2),
              (side,.115,h),wall)
    m.box((0,front,(h+door_h)/2),(door_w,.115,h-door_h),wall)
    m.box((0,front-.065,door_h/2),(door_w*.90,.035,door_h*.97),door)
    m.box((0,front-.092,door_h*.49),(door_w*.99,.035,.06),"beam")
    m.box((door_w*.28,front-.10,door_h*.51),(.027,.025,.04),"bronze")
    for x in (-door_w/2,door_w/2):
        m.box((x,front-.08,door_h/2),(.075,.10,door_h+.09),"beam")
    m.box((0,front-.09,door_h+.035),(door_w+.15,.11,.085),"beam")

def windows(m,w,d,h,style="plain"):
    # Set windows back behind visible jambs rather than drawing marks on walls.
    side_y=0
    for x in (-w/2,w/2):
        outside=x+(.068 if x>0 else -.068)
        m.box((outside,side_y,h*.57),(.016,.29,.33),"iron",(.62,.69,.67,1))
        for yy in (-.17,.17):
            m.box((outside,yy,h*.57),(.07,.07,.45),"beam")
        m.box((outside,0,h*.39),(.10,.44,.07),"limestone")
        m.box((outside,0,h*.76),(.10,.43,.065),"beam")
        if style=="shutter":
            m.box((outside,side_y+.29,h*.57),(.08,.16,.32),"plank")
    m.box((0,d/2+.082,h*.58),(.34,.026,.30),"iron",(.66,.71,.70,1))
    for sx in (-.19,.19):
        m.box((sx,d/2+.10,h*.58),(.045,.07,.40),"beam")
    m.box((0,d/2+.10,h*.38),(.48,.10,.07),"limestone")

def house(name,w,d,wall_h,roof_cell,wall="plaster",half_timber=True,
          chimney=False,seed=0):
    m=Model(name,description="Recessed-door, timber-framed medieval dwelling")
    m.box((0,0,.055),(w+.14,d+.14,.11),"fieldstone")
    m.box((0,d/2,wall_h/2),(w,.11,wall_h),wall)
    for x in (-w/2,w/2):
        m.box((x,0,wall_h/2),(.11,d,wall_h),wall)
    doorway(m,w,d,wall_h,door_w=min(.67,w*.3),door_h=wall_h*.59,wall=wall)
    if half_timber: timber_frame(m,w,d,wall_h,.075)
    else:
        for y in (-d/2,d/2):
            for x in (-w/2,w/2):
                m.box((x,y,wall_h*.45),(.10,.11,wall_h*.9),"limestone")
    windows(m,w,d,wall_h,"shutter" if name in ("cottage","longhouse") else "plain")
    roof_gable(m,w,d,wall_h,.47 if name=="hut" else .54,roof_cell)
    if chimney:
        if name=="townhouse":
            # Runtime smoke anchor is local Bevy (x=.75,y=2.98,z=.20).
            cx,cy,top_y=.75,-.20,2.98
            bottom=wall_h+.05
            m.box((cx,cy,(bottom+top_y)/2),(.27,.29,top_y-bottom),"fieldstone")
            m.box((cx,cy,top_y-.045),(.38,.40,.09),"limestone")
            m.box((cx,cy,top_y+.005),(.16,.18,.010),"iron",(.30,.29,.28,1))
            # The upstairs shutter/light anchor is on the +Z (front) gable.
            m.box((.45,-d/2+.015,1.80),(.44,.045,.34),"plaster")
            m.box((.45,-d/2-.065,1.80),(.30,.025,.27),"iron",(.48,.48,.41,1))
            for sx in (.23,.67): m.box((sx,-d/2-.083,1.80),(.055,.08,.43),"beam")
        else:
            cx=w*.28
            m.box((cx,d*.15,wall_h+.24),(.22,.23,.53),"fieldstone")
            m.box((cx,d*.15,wall_h+.51),(.30,.31,.07),"limestone")
            m.box((cx,d*.15,wall_h+.53),(.14,.15,.012),"iron")
    # Wood/stone texture has mild age variation through linear vertex colour.
    return m

def masonry_wall(name,length,thick=.62,height=1.39):
    m=Model(name,description="Course-built defensive curtain wall with crenels")
    m.box((0,0,.08),(length,thick+.13,.16),"fieldstone")
    rng=random.Random(int(length*17))
    course_h=.25
    for row in range(5):
        z=.18+row*course_h
        cell="limestone" if row%3==1 else "fieldstone"
        offset=.34 if row%2 else 0
        x=-length/2
        while x<length/2-.01:
            block=min(length/2-x,.70+rng.uniform(-.14,.17))
            if x==-length/2 and offset:
                block=min(block,.43)
            if block<.03: break
            m.box((x+block/2,0,z),(block-.018,thick,course_h-.014),cell,
                  (rng.uniform(.88,1),)*3+(1,))
            x+=block
    m.box((0,0,height-.12),(length,thick+.12,.17),"limestone")
    count=int(length/.86)
    for i in range(count):
        x=-length/2+(i+.5)*length/count
        if i%2==0:
            m.box((x,0,height+.12),(length/count*.81,thick+.16,.28),"limestone")
    # Timber walk planks just inside the wall and stone drip-line.
    m.box((0,thick*.70,height-.26),(length-.22,.46,.08),"plank")
    m.box((0,-thick*.65,.35),(length,.11,.09),"limestone")
    return m

def tower(name="tower"):
    m=Model(name,description="Quoined stone corner tower, conical slate roof")
    w=2.05;d=2.05
    m.box((0,0,.11),(w+.16,d+.16,.22),"fieldstone")
    m.box((0,0,1.31),(w,d,2.35),"fieldstone")
    for x in (-w/2,w/2):
        for y in (-d/2,d/2):
            for k in range(9):
                m.box((x,y,.30+k*.24),(.17,.17,.21),"limestone")
    for level in (.95,1.70):
        for side in (-1,1):
            m.box((side*(w/2+.014),0,level),(.025,.14,.25),"iron")
            m.box((0,side*(d/2+.014),level),(.14,.025,.25),"iron")
    m.box((0,0,2.53),(w+.16,d+.16,.17),"limestone")
    for i in range(8):
        a=i*math.tau/8
        m.box(((w/2)*math.cos(a),(d/2)*math.sin(a),2.69),
              (.33,.33,.23),"limestone")
    # Eight-plane tapered roof with slight irregularity.
    rb=1.30;rt=.055
    for i in range(8):
        a=i*math.tau/8;b=(i+1)*math.tau/8
        m.quad((rb*math.cos(a),rb*math.sin(a),2.82),
               (rb*math.cos(b),rb*math.sin(b),2.82),
               (rt*math.cos(b),rt*math.sin(b),3.72),
               (rt*math.cos(a),rt*math.sin(a),3.72),"slate")
    m.cylinder((0,0,3.78),.065,.30,"bronze",8)
    # Native flag cloth pivots at local y=4.75; the pole is already present at every tower.
    m.cylinder((0,0,4.46),.04,1.08,"beam",8)
    m.cylinder((0,0,5.02),.075,.10,"bronze",8)
    return m

def keep(name="keep_core"):
    m=Model(name,description="Masonry keep with arched portal, corner buttresses and crenellated parapet")
    w=6.9;d=5.9;h=3.4
    m.box((0,0,.15),(w+.30,d+.30,.30),"fieldstone")
    # Four wall sections leave a true recessed front gateway opening.
    m.box((0,d/2-.11,h/2),(w,.24,h),"fieldstone")
    for x in (-w/2+.11,w/2-.11):
        m.box((x,0,h/2),(.24,d-.48,h),"fieldstone")
    portal=1.15
    for sx in (-1,1):
        side=(w-portal)/2
        m.box((sx*(portal/2+side/2),-d/2+.11,h/2),
              (side,.24,h),"fieldstone")
    m.box((0,-d/2+.11,(h+1.64)/2),(portal,.24,h-1.64),"fieldstone")
    m.box((0,-d/2-.012,.84),(portal*.94,.038,1.60),"plank")
    for sx in (-1,1):
        m.box((sx*.62,-d/2-.05,.86),(.17,.14,1.88),"limestone")
    m.box((0,-d/2-.07,1.79),(1.48,.16,.18),"limestone")
    m.box((0,-d/2-.055,1.0),(.87,.055,.045),"iron")
    for k in range(3):
        z=.73+k*.74
        for sx in (-1,1):
            m.box((sx*2.05,-d/2-.04,z),(.21,.04,.33),"iron")
            m.box((sx*2.05,-d/2-.10,z-.20),(.39,.13,.06),"limestone")
    for x in (-w/2,w/2):
        for y in (-d/2,d/2):
            for k in range(11):
                m.box((x,y,.36+k*.27),(.25,.25,.25),"limestone")
    m.box((0,0,h+.03),(w+.30,d+.30,.20),"limestone")
    for axis,extent,count in (("x",w+ .3,9),("y",d+.3,8)):
        for i in range(count):
            p=-extent/2+(i+.5)*extent/count
            if i%2:
                for sign in (-1,1):
                    pos=(p,sign*(d/2+.04),h+.22) if axis=="x" else (sign*(w/2+.04),p,h+.22)
                    size=(extent/count*.78,.31,.34) if axis=="x" else (.31,extent/count*.78,.34)
                    m.box(pos,size,"limestone")
    for x in (-2.5,2.5):
        m.box((x,0,h*.73),(.025,.25,.38),"iron")
    # This central turret and its pole belong to the always-visible core. Tier 2
    # only adds gilding and heraldry, so the pennant never hangs over empty air.
    m.box((0,0,3.61),(2.0,2.0,.42),"limestone")
    m.box((0,0,3.80),(2.16,2.16,.14),"fieldstone")
    roof_gable(m,2.12,2.12,3.85,.34,"slate",over=.05)
    m.cylinder((0,0,4.64),.045,.96,"beam",8)
    m.cylinder((0,0,5.15),.09,.09,"bronze",8)
    return m

def keep_tier(name,tier):
    m=Model(name,description="Additive keep grandeur tier")
    if tier==1:
        m.box((0,0,.008),(.025,.025,.016),"fieldstone")
        for sx in (-1,1):
            for sy in (-1,1):
                x=sx*3.30;y=sy*2.80
                m.box((x,y,3.05),(.74,.74,1.70),"fieldstone")
                m.box((x,y,3.96),(.90,.90,.16),"limestone")
                m.cylinder((x,y,4.32),.45,.62,"slate",4,top_radius=.02)
                m.cylinder((x,y,4.68),.095,.12,"bronze",7)
    else:
        # Registration stud keeps the additive mesh's origin at the keep base.
        m.box((0,0,.008),(.025,.025,.016),"fieldstone")
        m.box((0,0,3.74),(2.22,2.22,.16),"bronze",(.88,.71,.36,1))
        for sx in (-1,1):
            for sy in (-1,1):
                m.cylinder((sx*3.30,sy*2.80,4.77),.29,.24,"bronze",4,top_radius=.015)
        for x in (-1.70,1.70):
            m.box((x,-3.05,2.05),(.95,.06,2.50),"burgundy_cloth")
    return m

def gate():
    m=Model("gate",description="Four-unit arched castle gate with ironbound timber doors")
    for sx in (-1,1):
        x=sx*2.08
        m.box((x,0,.13),(.76,1.0,.26),"fieldstone")
        m.box((x,0,1.02),(.62,.85,1.67),"fieldstone")
        for z in (.33,.74,1.15,1.56):
            m.box((x,0,z),(.68,.93,.09),"limestone")
        m.box((x,0,1.92),(.83,1.12,.18),"limestone")
        m.box((x,0,2.13),(.62,.86,.27),"fieldstone")
        m.box((x,0,2.33),(.72,.98,.14),"limestone")
    m.box((0,0,1.94),(3.72,.83,.23),"beam")
    m.box((0,0,2.23),(4.12,.95,.15),"plank")
    for x in (-1.86,1.86):
        m.box((x,-.47,.96),(.12,.08,1.58),"iron")
    return m

def fort_variants():
    models=[]
    # Canonical four dwellings preserve the existing slot collision footprints.
    models.extend((
        house("hut",2,1.7,1.23,"thatch",wall="wattle",half_timber=False),
        house("cottage",2.6,2,1.45,"thatch",chimney=True),
        house("townhouse",2.3,1.9,1.65,"clay_roof",chimney=True),
        house("longhouse",3.6,1.8,1.48,"slate",chimney=True),
        keep(),keep_tier("keep_tier1",1),keep_tier("keep_tier2",2),
        masonry_wall("wall_15",15),masonry_wall("wall_10",10),
        tower(),gate(),
    ))
    return models

def town_structures():
    m=[]
    farm=house("farm",2.1,2.2,1.25,"thatch",wall="wattle",half_timber=True)
    for x in (-1.18,-.83):
        farm.box((x,1.25,.36),(.12,.13,.72),"beam")
    farm.box((-1.0,1.25,.72),(.55,.15,.09),"beam")
    m.append(farm)
    saw=house("sawmill",2.5,2.0,1.35,"slate",wall="plank",half_timber=False)
    for x in (-.9,-.46,-.02,.42):
        saw.beam((x,1.1,.14),(x,1.68,.14),.20,.18,"beam")
    m.append(saw)
    mine=Model("mine",description="Timbered mine portal cut into sculpted rock face")
    mine.boulder((-.55,.15,0),1.04,"granite",seed=49)
    mine.boulder((.65,.20,0),1.08,"fieldstone",seed=50)
    mine.box((0,-.52,.93),(1.40,.31,1.87),"fieldstone")
    mine.box((0,-.72,.81),(.86,.04,1.38),"iron")
    for x in (-.53,.53):
        mine.box((x,-.80,.85),(.17,.18,1.48),"beam")
    mine.box((0,-.80,1.57),(1.32,.18,.18),"beam")
    m.append(mine)
    plot=Model("plot",description="Stone-edged tilled producer plot")
    plot.box((0,0,.025),(4,3,.05),"earth")
    for x in (-2,2): plot.box((x,0,.11),(.12,3.14,.18),"fieldstone")
    for y in (-1.5,1.5): plot.box((0,y,.11),(4.12,.12,.18),"fieldstone")
    for row in range(3):
        plot.box((.3,-.7+row*.7,.105),(2.9,.32,.07),"earth",(.75,.60,.48,1))
    m.append(plot)
    site=Model("house_site",description="Masonry footing and sawn delivery timber")
    site.box((0,0,.09),(2.55,2.1,.18),"fieldstone")
    for x in (-1.05,1.05):
        for y in (-.84,.84):
            site.box((x,y,.38),(.16,.16,.58),"beam")
    site.box((.45,-.67,.17),(.93,.24,.12),"plank")
    m.append(site)
    for name,seed in (("rubble_farm",23),("rubble_sawmill",29),("rubble_mine",31)):
        rubble=Model(name,description="Scattered broken masonry and timber rubble")
        rng=random.Random(seed)
        for i in range(11):
            x=rng.uniform(-1.25,1.25);y=rng.uniform(-.95,.95)
            rubble.boulder((x,y,0),rng.uniform(.11,.30),
                           "fieldstone" if i%2 else "limestone",seed+i)
        for i in range(4):
            x=rng.uniform(-1.1,1.1);y=rng.uniform(-.8,.8)
            rubble.beam((x,y,.1),(x+rng.uniform(.2,.7),y+.11,.13),.08,.07,"beam")
        m.append(rubble)
    return m

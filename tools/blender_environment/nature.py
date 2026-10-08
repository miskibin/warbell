"""Biomes, rock landmarks and lightweight alpha-cutout understory."""
import math
import random
from envkit import Model

def clump(name,cell,height,width,count,seed,base_color=(1,1,1,1)):
    rng=random.Random(seed)
    m=Model(name,"cutout",description="Blender-authored crossed botanical cards with varied stems")
    for i in range(count):
        a=math.tau*i/count+rng.uniform(-.22,.22)
        r=rng.uniform(0,width*.24)
        h=height*rng.uniform(.72,1.10)
        w=width*rng.uniform(.60,1.12)
        col=tuple(min(1,c*rng.uniform(.80,1.10)) for c in base_color[:3])+(1,)
        m.card((r*math.cos(a),r*math.sin(a),0),w,h,cell,a,col)
        if i%3==0: m.card((r*math.cos(a),r*math.sin(a),0),w*.77,h*.85,cell,a+math.pi/2,col)
    return m

def tuft(name,height,spread,seed,straw=False):
    """Several offset blade fans instead of coincident billboard crosses."""
    rng=random.Random(seed)
    m=Model(name,"cutout",description="Natural patch of separate narrow grass blade fans")
    for group in range(3 if name!="grass_d" else 4):
        a=group*math.tau/(3 if name!="grass_d" else 4)+rng.uniform(-.25,.25)
        rr=spread*(.18+.12*(group%2))
        x,y=rr*math.cos(a),rr*math.sin(a)
        for k in range(3):
            h=height*rng.uniform(.58,1.08)
            w=spread*rng.uniform(.39,.62)
            col=(.70,.82,.57,1) if straw else ((.68,.92,.62,1) if k%2 else (.84,.97,.71,1))
            m.card((x+rng.uniform(-.035,.035),y+rng.uniform(-.035,.035),0),
                   w,h,"grass",a+k*math.pi/3,col)
    return m

def shrub(name,height,radius,seed,hue=(.85,.97,.75,1)):
    """Dense small-leaf sprigs on an irregular, visibly branching crown."""
    rng=random.Random(seed)
    m=Model(name,"cutout",description="Irregular branched shrub with many fine leafy sprays and twig gaps")
    m.cylinder((0,0,height*.17),.052,height*.34,"wood",8,top_radius=.027)
    for group in range(15):
        a=group*math.tau/15+rng.uniform(-.14,.14)
        rr=radius*rng.uniform(.68,.97)
        tip_z=height*rng.uniform(.48,.77)
        x,y=rr*math.cos(a),rr*math.sin(a)
        m.beam((0,0,height*.21),(x*.82,y*.82,tip_z),.027,.019,"wood")
        for k in range(7):
            t=.31+.10*k
            side=(-1 if k%2 else 1)
            lateral=side*radius*rng.uniform(.04,.14)
            cx=x*t-math.sin(a)*lateral
            cy=y*t+math.cos(a)*lateral
            base_z=height*(.18+.39*t)+rng.uniform(-.025,.025)
            w=radius*rng.uniform(.31,.46)
            h=height*rng.uniform(.22,.36)
            col=tuple(min(1,c*rng.uniform(.79,1.08)) for c in hue[:3])+(1,)
            m.card((cx,cy,base_z),w,h,"shrub",a+side*.47,col)
    # Interleaved crown sprays avoid a hollow centre without a single large sheet.
    for k in range(16):
        a=k*math.tau/16+.22
        m.card((radius*.20*math.cos(a),radius*.20*math.sin(a),height*.56),
               radius*.32,height*.25,"shrub",a+.67,hue)
    if name=="shrub_c":
        for i in range(9):
            a=i*math.tau/9+.31
            rr=radius*(.46+.11*(i%2))
            m.card((rr*math.cos(a),rr*math.sin(a),height*.46),.055,.06,
                   "flower",a,(.87,.30,.27,1))
    return m

def clover(name,seed,hue=(.74,.95,.65,1)):
    rng=random.Random(seed)
    m=Model(name,"cutout",description="Low three-leaf clover rosettes spread into a coherent meadow mat")
    for plant in range(7):
        a=plant*math.tau/7+rng.uniform(-.17,.17)
        rr=.08+.13*(plant%3)/2
        x,y=rr*math.cos(a),rr*math.sin(a)
        stem_h=.07+rng.uniform(0,.06)
        m.beam((x,y,0),(x,y,stem_h),.012,.012,"wood",(.55,.83,.42,1))
        for leaf in range(3):
            ang=a+leaf*math.tau/3
            px=x+.055*math.cos(ang);py=y+.055*math.sin(ang)
            m.card((px,py,stem_h*.66),.16,.11,"shrub",ang+.45,hue)
    return m

def fern_rosette(name,seed,height=.38,spread=.34):
    rng=random.Random(seed)
    m=Model(name,"cutout",description="Layered radial fern fronds with arching tips")
    m.beam((0,0,0),(0,0,height*.54),.025,.021,"wood",(.52,.71,.39,1))
    for i in range(11):
        a=i*math.tau/11+rng.uniform(-.10,.10)
        length=spread*rng.uniform(.71,1.10)
        low=(.025+.008*(i%3))
        peak=height*rng.uniform(.56,1.00)
        tx,ty=-math.sin(a),math.cos(a)
        w=.085+rng.uniform(0,.045)
        m.quad((tx*w/2,ty*w/2,low),(-tx*w/2,-ty*w/2,low),
               (length*math.cos(a)-tx*w*.42,length*math.sin(a)-ty*w*.42,peak),
               (length*math.cos(a)+tx*w*.42,length*math.sin(a)+ty*w*.42,peak),
               "fern",(.66,.86,.56,1))
    return m

def flower_patch(name,seed,height,hue,spread=.25,count=5):
    rng=random.Random(seed)
    m=Model(name,"cutout",description="Wildflower patch with separated colored heads and green stems")
    for i in range(count):
        a=i*math.tau/count+rng.uniform(-.20,.20)
        rr=spread*rng.uniform(.25,.87)
        x,y=rr*math.cos(a),rr*math.sin(a)
        h=height*rng.uniform(.68,1.07)
        w=.13+rng.uniform(0,.09)
        n=len(m.colors)
        m.card((x,y,0),w,h,"flower",a+.45,(1,1,1,1))
        bottom=(.51,.81,.42,1)
        top=tuple(float(v) for v in hue)+(1,)
        m.colors[n:n+4]=[bottom,bottom,top,top]
        if i%2==0:
            m.card((x+.025,y-.018,.03),w*.63,h*.44,"shrub",a+math.pi/2,
                   (.57,.80,.44,1))
    return m

def litter_patch(name,seed,color):
    rng=random.Random(seed)
    m=Model(name,"cutout",description="Layered autumn leaves, needles and broken twigs on the soil")
    for i in range(11):
        a=i*2.40+rng.uniform(-.2,.2)
        rr=.07+.20*((i*5)%11)/11
        x,y=rr*math.cos(a),rr*math.sin(a)
        sx=.13+rng.uniform(0,.09);sy=.07+rng.uniform(0,.05)
        c=tuple(min(1,v*rng.uniform(.75,1.10)) for v in color)+(1,)
        m.quad((x-sx,y-sy,.004),(x+sx,y-sy,.004),
               (x+sx,y+sy,.004),(x-sx,y+sy,.004),"leaf",c)
    for i in range(3):
        a=rng.uniform(0,math.tau)
        x,y=.15*math.cos(a),.15*math.sin(a)
        m.beam((x-.12*math.cos(a),y-.12*math.sin(a),.009),
               (x+.12*math.cos(a),y+.12*math.sin(a),.009),
               .012,.010,"wood",(.60,.49,.34,1))
    return m

def twig_tree(name,height=1.8,with_leaves=False,seed=0,snow=False,swamp=False):
    rng=random.Random(seed)
    m=Model(name,"cutout" if with_leaves else "opaque",
            description="Tapered sculptural trunk and grown branch whorls")
    bark=("pine_bark" if snow else "oak_bark") if with_leaves else "fieldstone"
    if not with_leaves: bark="beam"
    m.cylinder((0,0,height*.23),.085,height*.46,bark,9,top_radius=.064)
    m.cylinder((.035,0,height*.61),.064,height*.78,bark,8,top_radius=.013)
    if swamp:
        for i in range(5):
            a=i*math.tau/5
            m.beam((0,0,.36),(.31*math.cos(a),.31*math.sin(a),0),.075,.05,bark)
    for row in range(5 if snow else 7):
        z=height*(.32+row*(.10 if snow else .08))
        radius=(.58 if snow else .43)*(1-row/(7 if snow else 9))
        count=6 if snow else 5
        for i in range(count):
            a=(i+row*.4)*math.tau/count+rng.uniform(-.15,.15)
            r=radius*rng.uniform(.83,1.15)
            tip=(r*math.cos(a),r*math.sin(a),z+(-.08 if snow else .08))
            m.beam((0,0,z),(r*.53*math.cos(a),r*.53*math.sin(a),z+.02),
                   .032,.023,bark)
            m.beam((r*.53*math.cos(a),r*.53*math.sin(a),z+.02),tip,.020,.013,bark)
            if with_leaves:
                card="snow_needles" if snow else "leaf"
                col=(.78,.90,.85,1) if snow else ((.48,.68,.42,1) if swamp else (.70,.86,.50,1))
                for k in range(2):
                    rr=r*(.62+k*.25)
                    m.card((rr*math.cos(a),rr*math.sin(a),z-.09),
                           .32 if snow else .27,.29 if snow else .26,
                           card,a+k*.65,col)
    return m

def snow_pine(name,seed,height=2.45,spread=.72,layers=5):
    rng=random.Random(seed)
    m=Model(name,"cutout",description="Frost-laden conifer with full staggered branch tiers")
    m.cylinder((0,0,height*.29),.13,height*.58,"pine_bark",9,top_radius=.075)
    m.cylinder((.035,0,height*.72),.078,height*.56,"pine_bark",8,top_radius=.008)
    for row in range(layers):
        z=height*(.20+row*.145)
        radius=spread*(1-row/(layers+.15))
        for i in range(8):
            a=(i+row*.43)*math.tau/8+rng.uniform(-.10,.10)
            r=radius*rng.uniform(.82,1.13)
            x,y=r*math.cos(a),r*math.sin(a)
            m.beam((0,0,z+.06),(x*.95,y*.95,z-.12),.045,.035,"pine_bark")
            for k,rr in enumerate((.50,.75,.95)):
                xx,yy=x*rr,y*rr
                w=.36*(1-row*.085)*rng.uniform(.90,1.15)
                h=.27*(1-row*.065)*rng.uniform(.90,1.18)
                m.card((xx,yy,z-.18+h*.14),w,h,"snow_needles",
                       a+k*.72,(.92,.97,1,1))
                if k==1 and row<layers-1:
                    m.card((xx,yy,z-.20),w*.82,h*.76,"snow_needles",
                           a+math.pi/2,(.79,.89,1,1))
    # Frost pocket at the trunk foot roots the species in the snowy biome.
    for i in range(4):
        a=i*math.tau/4+.25
        m.card((.13*math.cos(a),.13*math.sin(a),0),.30,.17,
               "snow_needles",a,(.91,.96,1,1))
    return m

def snow_birch():
    rng=random.Random(203)
    m=Model("snow_birch_a","cutout",description="White-barked winter birch with fine bare branching silhouette")
    m.cylinder((0,0,.78),.086,1.56,"birch_bark",9,top_radius=.064)
    m.cylinder((.06,.01,1.88),.064,1.05,"birch_bark",8,top_radius=.012)
    for row in range(9):
        z=.46+row*.185
        for k in range(3):
            a=(k+row*.47)*math.tau/3+rng.uniform(-.20,.20)
            r=.49*(1-row/11)*rng.uniform(.8,1.2)
            tip=(r*math.cos(a),r*math.sin(a),z+.17)
            m.beam((.02,0,z),(tip[0]*.52,tip[1]*.52,z+.08),.029,.023,"birch_bark")
            m.beam((tip[0]*.52,tip[1]*.52,z+.08),tip,.015,.011,"birch_bark")
            if (row+k)%4==0:
                m.card((tip[0],tip[1],tip[2]-.10),.18,.21,"snow_twig",a,
                       (.87,.93,1,1))
    return m

def mangrove(name,seed,height=2.30,spread=1.08,lean=.18):
    rng=random.Random(seed)
    m=Model(name,"cutout",description="Crooked buttressed swamp tree with broad crown and hanging moss")
    m.cylinder((0,0,.26),.16,.52,"oak_bark",9,top_radius=.12)
    m.beam((0,0,.44),(lean*.60,-.06,height*.64),.16,.12,"oak_bark")
    m.beam((lean*.60,-.06,height*.64),(lean,.02,height*.91),.105,.075,"oak_bark")
    for i in range(7):
        a=i*math.tau/7+.23
        m.beam((0,0,.37),(.43*math.cos(a),.43*math.sin(a),0),.10,.075,"oak_bark")
    for row in range(3):
        z=height*(.56+row*.135)
        rad=spread*(1-row*.17)
        for i in range(9):
            a=(i+row*.37)*math.tau/9+rng.uniform(-.15,.15)
            r=rad*rng.uniform(.69,1.08)
            x=lean+r*math.cos(a);y=r*math.sin(a)
            m.beam((lean*.7,0,z-.12),(x*.82,y*.82,z+.025),.052,.04,"oak_bark")
            for k in range(3):
                rr=.63+.18*k
                col=(.67,.84,.55,1) if k%2 else (.49,.72,.44,1)
                m.card((lean+(x-lean)*rr,y*rr,z-.15),.42,.34,"leaf",a+k*.63,col)
            if i%3==0:
                m.card((x*.87,y*.87,z-.44),.16,.39,"fern",a+.5,
                       (.40,.67,.40,1))
    return m

def cypress_stump(name,seed,height=.92):
    rng=random.Random(seed)
    m=Model(name,"cutout",description="Broken swamp cypress stump with flared pneumatophores and moss")
    m.cylinder((0,0,height*.42),.19,height*.84,"oak_bark",9,top_radius=.12)
    m.cylinder((0,0,height*.87),.13,.05,"wood",9)
    for i in range(7):
        a=i*math.tau/7
        m.beam((.10*math.cos(a),.10*math.sin(a),.36),
               ((.40+.08*(i%2))*math.cos(a),(.40+.08*(i%2))*math.sin(a),0),
               .09,.075,"oak_bark")
        if i%2==0:
            m.beam((.23*math.cos(a),.23*math.sin(a),0),
                   ((.48+.03*i)*math.cos(a),(.48+.03*i)*math.sin(a),.23+rng.uniform(0,.1)),
                   .038,.030,"oak_bark")
    for i in range(8):
        a=i*math.tau/8+.15
        m.card((.18*math.cos(a),.18*math.sin(a),.25),.29,.31,
               "fern",a,(.42,.70,.44,1))
    return m

def windpine(name,seed,height=2.20,lean=.58):
    rng=random.Random(seed)
    m=Model(name,"cutout",description="Wind-bent living pine with one-sided needle crown")
    m.cylinder((0,0,.22),.12,.44,"pine_bark",9,top_radius=.10)
    m.beam((0,0,.36),(lean*.52,0,height*.58),.12,.095,"pine_bark")
    m.beam((lean*.52,0,height*.58),(lean,0,height*.96),.085,.055,"pine_bark")
    for row in range(4):
        z=height*(.47+row*.135)
        radius=.72*(1-row*.17)
        for k in range(7):
            a=(k+row*.37)*math.tau/7+rng.uniform(-.12,.12)
            x=lean*(.55+row*.10)+radius*math.cos(a)
            y=radius*.66*math.sin(a)
            m.beam((lean*.48,0,z-.04),(x*.83,y*.83,z-.13),.036,.025,"pine_bark")
            for j in range(2):
                m.card((x*(.7+.19*j),y*(.7+.19*j),z-.25),.35,.32,
                       "snow_needles",a+j*.81,(.43,.66,.39,1))
    return m

def cactus():
    m=Model("cactus_a",description="Ribbed saguaro cactus with curved segmented arms")
    m.cylinder((0,0,.76),.115,1.52,"wattle",10,top_radius=.094,
               colors=(.38,.72,.42,1))
    m.cylinder((0,0,1.55),.095,.22,"wattle",10,top_radius=.015,
               colors=(.40,.72,.42,1))
    for sx,level,reach in ((-1,.64,.34),(1,.98,.31)):
        m.beam((sx*.08,0,level),(sx*reach,0,level+.02),.10,.10,"wattle",(.37,.68,.40,1))
        m.cylinder((sx*reach,0,level+.22),.065,.42,"wattle",8,top_radius=.055,
                   colors=(.42,.71,.43,1))
    return m

def stump(name="stump_a"):
    m=Model(name,description="Flared weathered stump with radial root buttresses")
    m.cylinder((0,0,.18),.21,.36,"beam",11,top_radius=.18)
    m.cylinder((0,0,.365),.185,.04,"plank",11)
    for i in range(6):
        a=i*math.tau/6
        m.beam((.13*math.cos(a),.13*math.sin(a),.18),
               (.33*math.cos(a),.33*math.sin(a),0),.075,.06,"beam")
    return m

def rock(name,scale,seed):
    m=Model(name,description="Asymmetric triangulated glacial stone group with moss seams")
    if scale<=.55:
        for i,(x,y,rr) in enumerate(((-.11,.03,.68),(.16,-.10,.51),(.09,.16,.43),(-.20,-.14,.32))):
            m.boulder((x*scale/.34,y*scale/.34,0),scale*rr,
                      "granite" if i%3 else "fieldstone",seed+i*13,
                      (.80+.05*(i%2),.83,.79,1))
        # Irregular moss plates hug the two broadest stone caps.
        for x,y,z,w in ((-.12,.04,scale*.62,.14),(.14,-.10,scale*.48,.10)):
            m.quad((x-w,y-w*.36,z),(x+w,y-w*.32,z+.008),
                   (x+w*.8,y+w*.45,z+.02),(x-w*.7,y+w*.43,z+.014),
                   "fieldstone",(.42,.61,.36,1))
    else:
        m.boulder((0,0,0),scale,"granite",seed)
        m.boulder((scale*.48,-scale*.35,0),scale*.46,"fieldstone",seed+11)
    return m

def mushroom(name="mushroom_a",seed=0,cap_cell="clay_roof",cap_color=(.87,.69,.58,1)):
    m=Model(name,description="Irregular clustered fleshy mushrooms with textured caps")
    rng=random.Random(seed)
    placements=((0,0,1),(.20,.09,.62),(-.13,.12,.44),(.12,-.17,.48))
    for x,y,s in placements:
        s*=rng.uniform(.85,1.12)
        m.cylinder((x,y,.12*s),.035*s,.24*s,"plaster",7)
        m.cylinder((x,y,.25*s),.14*s,.08*s,cap_cell,9,top_radius=.03*s,
                   colors=cap_color)
        if name=="mushroom_a":
            for a in range(5):
                ang=a*math.tau/5
                m.box((x+.08*s*math.cos(ang),y+.08*s*math.sin(ang),.30*s),
                      (.017*s,.017*s,.008*s),"plaster")
    return m

def snowdrift():
    m=Model("snowdrift_a",description="Wind-smoothed irregular snowbank with blue shadow edge")
    # Three staggered rings rather than a scaled hemisphere.
    rng=random.Random(47)
    ring=[]
    for i in range(12):
        a=i*math.tau/12
        r=1+.14*math.sin(i*2.7)
        ring.append((.85*r*math.cos(a),.43*r*math.sin(a),0))
    upper=[]
    for i,p in enumerate(ring):
        upper.append((p[0]*.70,p[1]*.72,.16+.035*math.sin(i*3)))
    for i in range(12):
        j=(i+1)%12
        m.quad(ring[i],ring[j],upper[j],upper[i],"plaster",(.87,.93,1,1))
        m.quad(upper[i],upper[j],(0,0,.29),(0,0,.29),"plaster",(.97,1,1,1))
    return m

def rock_landmark(name,height,radius,seed):
    rng=random.Random(seed)
    m=Model(name,description="Layered eroded sandstone/rock landmark")
    layers=7
    ring=[]
    for j in range(layers+1):
        z=height*j/layers
        taper=(1-.52*j/layers) if "spire" in name else (.92+.18*math.sin(j*.8))
        row=[]
        for i in range(10):
            a=i*math.tau/10
            rr=radius*taper*(1+.12*math.sin(i*2.14+j*.73+seed))
            row.append((rr*math.cos(a),rr*math.sin(a),z))
        ring.append(row)
    for j in range(layers):
        for i in range(10):
            k=(i+1)%10
            m.quad(ring[j][i],ring[j][k],ring[j+1][k],ring[j+1][i],
                   "granite" if "spire" in name else "earth",
                   (.89+.07*math.sin(j),)*3+(1,))
    return m

def arch():
    m=Model("arch_a",description="Eroded natural rock arch with walk-through aperture")
    for sx in (-1,1):
        m.boulder((sx*1.22,0,0),.88,"earth",41 if sx<0 else 49)
        m.beam((sx*1.20,0,.55),(sx*.91,0,2.25),.59,.68,"earth")
    m.beam((-1.08,0,2.20),(1.08,0,2.27),.59,.66,"earth")
    m.boulder((0,0,2.06),.53,"earth",97)
    return m

def meadow_grass_patch(name,seed,tall=False):
    """A full, irregular meadow mat for bounded near-camera chunk scattering."""
    rng=random.Random(seed)
    m=Model(name,"cutout",description="Dense mixed-height fine grass patch for a continuous meadow carpet")
    for i in range(27 if tall else 24):
        a=i*2.399963+rng.uniform(-.23,.23)
        rr=math.sqrt((i+.4)/28)*rng.uniform(.33,.47)
        x,y=rr*math.cos(a),rr*math.sin(a)
        h=rng.uniform(.25,.50) if not tall else rng.uniform(.32,.61)
        w=rng.uniform(.20,.29)
        green=(.88+rng.uniform(-.07,.07),.96+rng.uniform(-.04,.04),
               .80+rng.uniform(-.08,.08),1)
        m.card((x,y,0),w,h,"grass",a+.47,green)
        if i%6==0:
            m.card((x+.025,y-.014,0),w*.68,h*.74,"grass",a+1.8,
                   (.94,.96,.78,1))
    if tall:
        for i in range(5):
            a=i*math.tau/5+.22
            rr=.11+.22*(i%3)/3
            m.card((rr*math.cos(a),rr*math.sin(a),0),.065,.46+.035*(i%3),
                   "reed",a+.41,(.79,.78,.56,1))
    return m

def meadow_flower_patch(name,seed,hue):
    """Small flowering stems through grass, with staggered heights and colours."""
    rng=random.Random(seed)
    m=Model(name,"cutout",description="Dense wildflower-and-grass mat with many small separated blossoms")
    for i in range(24):
        a=i*2.399963+rng.uniform(-.20,.20)
        rr=math.sqrt((i+.3)/21)*rng.uniform(.31,.45)
        x,y=rr*math.cos(a),rr*math.sin(a)
        h=rng.uniform(.22,.42)
        m.card((x,y,0),rng.uniform(.18,.25),h,"grass",a+.35,
               (.87+rng.uniform(-.07,.07),.97+rng.uniform(-.04,.03),
                .79+rng.uniform(-.07,.07),1))
    for i in range(20):
        a=i*2.399963+.44+rng.uniform(-.18,.18)
        rr=math.sqrt((i+.3)/21)*rng.uniform(.28,.43)
        x,y=rr*math.cos(a),rr*math.sin(a)
        h=rng.uniform(.29,.56)
        color=tuple(min(1,max(0,c*rng.uniform(.87,1.08))) for c in hue)+(1,)
        m.card((x,y,0),rng.uniform(.09,.14),h,"flower",a+.3,color)
    return m

def nature_models():
    out=[
        tuft("grass_a",.35,.37,100),
        tuft("grass_b",.23,.35,101),
        tuft("grass_c",.42,.34,102),
        tuft("grass_d",.30,.39,103,straw=True),
        meadow_grass_patch("meadow_grass_a",301),
        meadow_grass_patch("meadow_grass_b",302,tall=True),
        meadow_flower_patch("meadow_flowers_yellow",303,(1,.88,.32)),
        meadow_flower_patch("meadow_flowers_white",304,(.98,.98,.92)),
        meadow_flower_patch("meadow_flowers_purple",305,(.75,.50,.94)),
        clover("clover_a",126),clover("clover_b",127,(.68,.87,.59,1)),
        clover("clover_c",128,(.82,.96,.68,1)),
        fern_rosette("fern_a",104,.36,.36),
        fern_rosette("fern_b",105,.25,.30),
        fern_rosette("fern_c",106,.42,.32),
        shrub("shrub_a",.66,.42,107,(.88,.98,.75,1)),
        shrub("shrub_b",.50,.46,108,(.66,.83,.57,1)),
        shrub("shrub_c",.60,.38,109,(.78,.90,.62,1)),
        flower_patch("flower_a",201,.35,(1,.90,.70)),
        flower_patch("flower_b",202,.30,(.94,.56,.62)),
        flower_patch("flower_c",203,.37,(.97,.91,.66)),
        flower_patch("flower_d",204,.33,(.87,.47,.38)),
        flower_patch("flower_e",205,.28,(.71,.66,.91)),
        flower_patch("flower_f",206,.39,(.51,.70,.96)),
        flower_patch("flower_g",207,.31,(.98,.97,.89)),
        flower_patch("flower_h",208,.29,(.82,.61,.92)),
        flower_patch("flower_i",209,.40,(.98,.67,.38)),
        clump("reed_a","reed",.76,.45,5,109),
        clump("dry_shrub_a","snow_twig",.63,.83,6,110,(.71,.59,.39,1)),
        clump("snow_shrub_a","snow_twig",.56,.83,6,111,(.93,.96,.92,1)),
        litter_patch("litter_a",112,(.69,.45,.27)),
        litter_patch("litter_b",113,(.74,.57,.34)),
        litter_patch("litter_c",114,(.51,.45,.30)),
        litter_patch("litter_d",115,(.83,.66,.36)),
        litter_patch("litter_e",116,(.57,.42,.33)),
        clump("crop_a","crop",.73,.62,6,113),
        rock("rock_a",.34,120),rock("rock_b",.40,121),rock("rock_c",.27,122),
        rock("boulder_a",.72,123),rock("boulder_b",.84,124),rock("boulder_c",.60,125),
        stump(),twig_tree("dead_tree_a",2.05,False,132),
        mangrove("swamp_tree_a",133,2.30,1.08,.18),
        mangrove("swamp_tree_b",134,2.06,.96,-.27),
        mangrove("swamp_tree_c",135,2.43,1.15,.36),
        cypress_stump("swamp_cypress_stump_a",136,.91),
        cypress_stump("swamp_cypress_stump_b",137,1.07),
        snow_pine("snow_pine_a",138,2.35,.81,5),
        snow_pine("snow_pine_b",139,2.28,.69,5),
        snow_pine("snow_pine_c",140,2.68,.61,6),
        snow_birch(),
        windpine("rock_windpine_a",141,2.25,.54),
        windpine("rock_windpine_b",142,2.05,.73),
        cactus(),mushroom(),
        mushroom("mushroom_b",151,"plaster",(.72,.59,.44,1)),
        mushroom("mushroom_c",152,"fieldstone",(.75,.70,.59,1)),
        mushroom("mushroom_d",153,"plaster",(.94,.90,.78,1)),
        snowdrift(),
        rock_landmark("rock_spire_a",2.6,.42,141),
        rock_landmark("mesa_a",3.4,2.7,142),arch(),
    ]
    frosted=rock("snow_boulder_a",.72,126)
    frosted.colors=[(.79,.86,.91,1) if i%3 else (.93,.96,1,1)
                    for i in range(len(frosted.colors))]
    out.append(frosted)
    ore=rock("ore_rock",.53,210)
    # Thin metallic seams and projecting crystals guide the native harvest glint.
    for i in range(5):
        a=i*math.tau/5
        ore.beam((.23*math.cos(a),.23*math.sin(a),.22),
                 (.35*math.cos(a),.35*math.sin(a),.43),
                 .035,.028,"iron",(.73,.80,.87,1))
    out.append(ore)
    out.append(clump("marsh_herb","fern",.42,.38,5,211,(.62,.87,.56,1)))
    return out

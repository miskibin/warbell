"""Distinct Blender-authored static silhouettes retained from all campaign biomes."""
import math
import random
from envkit import Model


def snow_stump_log():
    m=Model("snow_stump_log_a",description="Snow-capped sawn stump and adjacent fallen log")
    m.cylinder((0,0,.12),.14,.24,"beam",9,top_radius=.12)
    m.cylinder((0,0,.255),.125,.032,"plank",9)
    m.boulder((-.035,0,.26),.11,"plaster",71,(.94,.97,1,1))
    for a in range(5):
        t=a*math.tau/5
        m.beam((.09*math.cos(t),.09*math.sin(t),.13),
               (.19*math.cos(t),.19*math.sin(t),0),.055,.043,"beam")
    m.beam((-.25,.30,.085),(.28,.30,.085),.15,.15,"beam")
    for x,r in ((-.20,.065),(0,.085),(.19,.060)):
        m.boulder((x,.30,.145),r,"plaster",83+int((x+.2)*100),(.91,.95,1,1))
    return m


def snow_rocks(name,kind=0):
    m=Model(name,description="Fractured blue-grey stone with sculpted snow blanket")
    if kind==0:  # split tor
        m.boulder((-.10,0,0),.32,"granite",300,(.64,.72,.80,1))
        m.boulder((.11,-.03,.33),.23,"granite",301,(.72,.80,.87,1))
        m.box((.08,-.03,.65),(.38,.35,.075),"plaster",(.91,.96,1,1))
        m.beam((.31,-.10,.40),(.31,-.10,.23),.024,.023,"plaster",(.77,.91,1,1))
    else:
        offsets=((-0.25,.04,.25),(.19,-.12,.17),(.09,.22,.14))
        if kind==2: offsets=((-.21,-.06,.20),(.20,.10,.20),(-.02,.22,.15))
        for i,(x,y,r) in enumerate(offsets):
            m.boulder((x,y,0),r,"granite",320+kind*10+i,(.66+.04*i,.75,.83,1))
            m.box((x,y,r*.83),(r*1.15,r*.95,.035),"plaster",(.91,.96,1,1))
    return m


def snow_grass():
    m=Model("snow_grass_a","cutout",description="Winter grass tips emerging through sparse frosted thatch")
    for i in range(7):
        a=i*math.tau/7
        x=.10*math.cos(a);y=.10*math.sin(a)
        m.card((x,y,0),.14,.16+.045*(i%3),"grass",a,
               (.75,.75,.56,1) if i%2 else (.74,.87,.72,1))
        if i%3==0:
            m.card((x,y,0),.16,.09,"snow_needles",a+.7,(.92,.96,1,1))
    return m


def ice_glint():
    m=Model("ice_glint_a",description="Small angular ice crystals catching cold light")
    for i,(x,y,h) in enumerate(((0,0,.16),(.12,.08,.10),(-.11,-.05,.13))):
        m.cylinder((x,y,h*.50),.040,h,"plaster",4,top_radius=.004,
                   colors=(.69+.04*i,.87,.99,1))
    return m


def winter_litter(name,variant):
    m=Model(name,"cutout",description="Distinct holly, cone, icy shard or frosted twig ground litter")
    if variant==0:
        for i in range(5):
            a=i*math.tau/5
            m.card((.08*math.cos(a),.08*math.sin(a),0),.13,.11,"shrub",a,
                   (.49,.70,.49,1))
        for x,y in ((-.05,.02),(.03,-.02)):
            m.card((x,y,.05),.055,.065,"flower",0,(.92,.27,.22,1))
    elif variant==1:
        m.cylinder((0,0,.055),.065,.11,"wood",7,top_radius=.028)
        for i in range(5):
            a=i*math.tau/5
            m.card((.09*math.cos(a),.09*math.sin(a),0),.12,.10,
                   "snow_twig",a,(.86,.93,1,1))
    elif variant==2:
        for i in range(6):
            a=i*math.tau/6
            m.card((.07*math.cos(a),.07*math.sin(a),0),.14,.09,
                   "snow_needles",a,(.88,.95,1,1))
    else:
        for i in range(4):
            a=i*math.tau/4+.4
            m.beam((0,0,.012),(.21*math.cos(a),.21*math.sin(a),.02),
                   .018,.015,"wood",(.69,.61,.52,1))
            m.card((.18*math.cos(a),.18*math.sin(a),0),.10,.10,
                   "snow_twig",a,(.85,.90,.96,1))
    return m


def rock_cairn():
    m=Model("rock_cairn_a",description="Irregular hand-stacked mountain waymarker stones")
    for level in range(4):
        z=.07+level*.17
        r=.27-level*.045
        m.boulder((.035*math.sin(level*2.2),.025*math.cos(level*1.5),z),
                  r,"granite",420+level,(.80,.79,.74,1))
    for x,y in ((-.30,.05),(.24,-.15)):
        m.boulder((x,y,0),.13,"fieldstone",430+int((x+.3)*10))
    return m


def scree(name,seed):
    rng=random.Random(seed)
    m=Model(name,description="Directional spill of angular mountain scree")
    for i in range(12):
        t=i/11
        x=-.43+t*.90+rng.uniform(-.08,.08)
        y=(t-.5)*.36+rng.uniform(-.12,.12)
        r=(.13-.06*t)*rng.uniform(.72,1.24)
        m.boulder((x,y,0),r,"granite" if i%2 else "fieldstone",seed+i,
                  (.76+.08*(i%3),.78,.73,1))
    return m


def dry_tuft():
    m=Model("rock_dry_tuft_a","cutout",description="Low bleached tussock among mountain gravel")
    for i in range(8):
        a=i*math.tau/8
        m.card((.065*math.cos(a),.065*math.sin(a),0),.13,.19+.03*(i%3),
               "grass",a,(.78,.70,.42,1))
    return m


def rock_litter(name,variant):
    m=Model(name,"cutout",description="Lichen pebbles, slate flakes or tiny alpine flowers")
    if variant<2:
        for i in range(5+variant*2):
            a=i*2.4
            rr=.06+.035*i
            x=rr*math.cos(a);y=rr*math.sin(a)
            m.boulder((x,y,0),.06+.015*(i%3),"stone",500+variant*20+i,
                      (.67,.72,.62,1) if variant==0 else (.76,.75,.67,1))
    else:
        for i in range(4):
            a=i*math.tau/4
            x=.07*math.cos(a);y=.07*math.sin(a)
            m.card((x,y,0),.09,.18,"flower",a,
                   (.72,.66,.92,1) if i%2 else (.96,.96,.83,1))
    return m


def barrel_cactus(name,seed):
    m=Model(name,description="Ribbed barrel cactus with spines and small blossom crown")
    h=.34 if seed%2==0 else .40
    r=.28 if seed%2==0 else .24
    m.cylinder((0,0,h*.5),r,h,"wattle",10,top_radius=r*.89,
               colors=(.39,.72,.41,1))
    m.cylinder((0,0,h-.015),r*.87,.075,"wattle",10,top_radius=.12,
               colors=(.51,.78,.47,1))
    for i in range(10):
        a=i*math.tau/10
        x=r*.96*math.cos(a);y=r*.96*math.sin(a)
        m.beam((x,y,.045),(x*.93,y*.93,h*.87),.027,.023,"wattle",
               (.31,.61,.34,1))
        m.box((x,y,h*.65),(.018,.018,.045),"plaster",(.91,.87,.63,1))
    for i in range(5):
        a=i*math.tau/5
        m.boulder((.09*math.cos(a),.09*math.sin(a),h),.04,
                  "plaster",seed+i,(.93,.77,.37,1) if seed%2==0 else (.92,.50,.64,1))
    return m


def pear_pad(m,x,y,z,rx,rz,color):
    # Eight-sided flattened fleshy pad, thick enough to have a convincing rim.
    pts=[]
    for i in range(8):
        a=i*math.tau/8
        pts.append((x+rx*math.cos(a),z+rz*math.sin(a)))
    for side,yy in ((-1,y-.035),(1,y+.035)):
        for i in range(8):
            j=(i+1)%8
            a=(x,yy,z);b=(pts[i][0],yy,pts[i][1]);c=(pts[j][0],yy,pts[j][1])
            if side<0: m.quad(a,b,c,a,"wattle",color)
            else: m.quad(a,c,b,a,"wattle",color)
    for i in range(8):
        j=(i+1)%8
        m.quad((pts[i][0],y-.035,pts[i][1]),(pts[j][0],y-.035,pts[j][1]),
               (pts[j][0],y+.035,pts[j][1]),(pts[i][0],y+.035,pts[i][1]),"wattle",color)


def prickly_pear(name,flip):
    m=Model(name,description="Three-dimensional opuntia paddles, spines, fruit and bloom")
    pear_pad(m,0,0,.28,.24,.29,(.39,.72,.42,1))
    pear_pad(m,-.19*flip,-.025,.53,.19,.25,(.32,.65,.39,1))
    pear_pad(m,.20*flip,.035,.55,.18,.23,(.49,.76,.45,1))
    if flip>0: pear_pad(m,.03,-.01,.75,.13,.17,(.41,.70,.40,1))
    for x,z in ((-.20*flip,.76),(.20*flip,.79)):
        m.boulder((x,0,z),.055,"clay_roof",710+int((x+.3)*100),(.94,.43,.34,1))
    for x,z in ((-.08,.25),(.10,.40),(-.20*flip,.56),(.19*flip,.57)):
        m.box((x,-.05,z),(.020,.025,.022),"plaster",(.96,.91,.72,1))
    return m


def desert_grass():
    m=Model("desert_grass_a","cutout",description="Sparse sun-dried desert grass fan")
    for i in range(6):
        a=i*math.tau/6
        m.card((.045*math.cos(a),.045*math.sin(a),0),.11,.19+.025*(i%2),
               "grass",a,(.85,.76,.48,1))
    return m


def succulent():
    m=Model("desert_succulent_a",description="Low fleshy rosette with a small pink bloom")
    m.cylinder((0,0,.025),.055,.05,"wattle",7,colors=(.39,.67,.42,1))
    for i in range(7):
        a=i*math.tau/7
        x=.09*math.cos(a);y=.09*math.sin(a)
        m.beam((0,0,.02),(x,y,.07),.07,.044,"wattle",(.43,.73,.50,1))
    m.boulder((0,0,.09),.035,"plaster",811,(.95,.65,.73,1))
    return m


def desert_litter(name,variant):
    m=Model(name,"cutout",description="Brittlebush bloom, poppy or sun-bleached twig")
    if variant<2:
        count=4 if variant==0 else 1
        for i in range(count):
            a=i*math.tau/max(1,count)
            x=.045*math.cos(a);y=.045*math.sin(a)
            m.beam((x,y,0),(x,y,.08+.02*(i%2)),.012,.010,"wood",(.64,.62,.43,1))
            m.card((x,y,.05),.09,.08,"flower",a,
                   (.96,.64,.34,1) if variant==0 else (.98,.86,.40,1))
    else:
        for a in (.4,2.0,3.7):
            m.beam((-.10*math.cos(a),-.10*math.sin(a),.012),
                   (.15*math.cos(a),.15*math.sin(a),.012),.015,.013,
                   "wood",(.76,.69,.55,1))
    return m


def saguaro(name,height,arms):
    m=Model(name,description="Ribbed columnar desert saguaro with species-specific arm count")
    m.cylinder((0,0,height*.47),.12,height*.94,"wattle",10,top_radius=.085,
               colors=(.36,.70,.42,1))
    m.cylinder((0,0,height*.965),.085,.09,"wattle",10,top_radius=.006,
               colors=(.42,.74,.44,1))
    for i,(side,level,reach) in enumerate(arms):
        m.beam((side*.09,0,level),(side*reach,0,level+.04),.095,.08,"wattle",
               (.32,.65,.40,1))
        m.cylinder((side*reach,0,level+.23),.06,.43,"wattle",8,top_radius=.048,
                   colors=(.43,.72,.43,1))
    for i in range(8):
        a=i*math.tau/8
        x=.116*math.cos(a);y=.116*math.sin(a)
        m.beam((x,y,.12),(x*.74,y*.74,height*.85),.012,.011,"plaster",(.71,.80,.59,1))
    return m


def desert_skull():
    m=Model("desert_skull_a",description="Bleached animal cranium, dark sockets and scattered ribs")
    m.boulder((0,0,.06),.17,"plaster",860,(.91,.87,.77,1))
    m.box((0,-.15,.07),(.17,.15,.10),"plaster",(.83,.78,.66,1))
    for x in (-.07,.07):
        m.box((x,-.17,.14),(.055,.012,.050),"iron",(.27,.26,.23,1))
    for i,(x,y,a) in enumerate(((.25,.02,.4),(.30,-.14,-.5))):
        m.beam((x-.15*math.cos(a),y-.15*math.sin(a),.02),
               (x+.15*math.cos(a),y+.15*math.sin(a),.02),
               .04,.033,"plaster",(.87,.84,.75,1))
    return m


def bleached_rock(name,seed):
    rng=random.Random(seed)
    m=Model(name,description="Pale sun-bleached desert rock cluster with chipped sandstone facets")
    for i in range(4 if seed%2 else 3):
        a=i*math.tau/(4 if seed%2 else 3)+rng.uniform(-.22,.22)
        rr=(.08+.13*(i%2)) if i else 0
        x,y=rr*math.cos(a),rr*math.sin(a)
        r=(.16 if i else .28)*rng.uniform(.85,1.13)
        m.boulder((x,y,r*.54),r,"limestone",seed*10+i,
                  (.81+rng.uniform(-.04,.04),.78+rng.uniform(-.04,.04),.68+rng.uniform(-.04,.04),1))
    return m


def swamp_log():
    m=Model("swamp_log_a",description="Half-sunken broken log with snag limbs and moss saddle")
    m.beam((-.50,0,.11),(.42,.09,.16),.25,.22,"beam",(.58,.63,.53,1))
    for x in (-.45,.38):
        m.cylinder((x,.02,.12),.12,.04,"plank",8,colors=(.65,.60,.47,1))
    for x,y,z in ((-.22,0,.16),(.14,.05,.18)):
        m.beam((x,y,z),(x+.18,y-.09,z+.37),.055,.048,"beam")
    for i,(x,y,w) in enumerate(((-.31,-.06,.31),(.09,.07,.36))):
        m.quad((x-w/2,y-.09,.23),(x+w/2,y-.09,.25),
               (x+w*.44,y+.10,.28),(x-w*.48,y+.10,.26),
               "fieldstone",(.37,.62,.39,1))
    return m


def moss_patch():
    m=Model("moss_patch_a","cutout",description="Low two-tone moss carpet with brighter tufted lobes")
    for i in range(12):
        a=i*2.40
        rr=.06+.20*((i*7)%12)/12
        x=rr*math.cos(a);y=rr*math.sin(a)
        m.card((x,y,0),.16,.055+.022*(i%3),"shrub",a,
               (.43,.72,.39,1) if i%3 else (.61,.84,.46,1))
    return m


def biome_extra_models():
    return [
        snow_stump_log(),snow_rocks("snow_tor_a",0),
        snow_rocks("snow_boulder_huddle_a",1),snow_rocks("snow_boulder_huddle_b",2),
        snow_grass(),ice_glint(),
        *[winter_litter(f"winter_litter_{c}",i) for i,c in enumerate("abcd")],
        rock_cairn(),scree("rock_scree_a",511),scree("rock_scree_b",512),
        dry_tuft(),*[rock_litter(f"rock_litter_{c}",i) for i,c in enumerate("abc")],
        barrel_cactus("barrel_cactus_a",610),barrel_cactus("barrel_cactus_b",611),
        prickly_pear("prickly_pear_a",1),prickly_pear("prickly_pear_b",-1),
        desert_grass(),succulent(),
        *[desert_litter(f"desert_litter_{c}",i) for i,c in enumerate("abc")],
        saguaro("cactus_b",1.45,[(-1,.57,.35)]),
        saguaro("cactus_c",1.90,[(-1,.64,.35),(1,1.14,.38)]),
        bleached_rock("desert_bleached_rock_a",820),
        bleached_rock("desert_bleached_rock_b",821),
        desert_skull(),swamp_log(),moss_patch(),
    ]

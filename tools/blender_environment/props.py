"""Small textured medieval props and interactive structure shells."""
import math
import copy
import random
from envkit import Model
from architecture import house, roof_gable

def renamed(model,name,description=None):
    new=copy.deepcopy(model)
    new.name=name
    if description: new.description=description
    return new

def wheel(m,x,y,r=.28):
    m.cylinder((x,y,r),r,.08,"beam",12,top_radius=r)
    m.cylinder((x,y,r),r*.15,.11,"bronze",9)
    for i in range(6):
        a=i*math.tau/6
        m.beam((x,y,r),(x+r*.80*math.cos(a),y,r+r*.80*math.sin(a)),
               .035,.03,"beam")

def cart(name="cart"):
    m=Model(name,description="Two-wheel timber cart with separate axles, hub and plank bed")
    m.box((0,0,.39),(1.12,.63,.08),"plank")
    for x in (-.49,.49):
        m.box((x,0,.57),(.065,.66,.35),"beam")
    for y in (-.27,.27):
        m.box((0,y,.56),(1.05,.055,.31),"plank")
    for y in (-.42,.42):
        wheel(m,0,y,.29)
    m.beam((.46,0,.39),(1.18,0,.26),.055,.07,"beam")
    for x in (-.31,.30):
        m.box((x,0,.43),(.05,.69,.08),"iron")
    return m

def hay_bale(name="hay_bale"):
    m=Model(name,description="Bound straw bale with irregular stalk fringe")
    m.box((0,0,.25),(.55,.39,.50),"straw")
    for x in (-.13,.13):
        m.box((x,0,.26),(.026,.43,.53),"beam",(.65,.48,.30,1))
    for i in range(16):
        a=i*2.4
        x=.25*math.cos(a);y=.17*math.sin(a)
        m.beam((x,y,.37),(x+.06*math.cos(a),y+.04*math.sin(a),.48),
               .012,.012,"straw")
    return m

def chest(name="chest_body",relic=False):
    m=Model(name,description="Ironbound oak chest lower shell with raised panel face")
    m.box((0,0,.19),(.70,.50,.38),"plank")
    for y in (-.25,.25):
        m.box((0,y,.20),(.74,.035,.39),"beam")
        for x in (-.31,.31):
            m.box((x,y+(-.02 if y<0 else .02),.20),(.055,.035,.38),
                  "bronze" if relic else "iron")
    for x in (-.31,.31):
        m.box((x,0,.20),(.06,.54,.40),"iron")
    m.box((0,-.268,.21),(.14,.04,.14),"bronze" if relic else "iron")
    m.box((0,-.294,.19),(.035,.01,.044),"iron")
    if relic:
        for x in (-.19,.19):
            m.box((x,-.287,.20),(.07,.025,.24),"bronze")
    return m

def chest_lid(name="chest_lid",relic=False):
    m=Model(name,description="Back-edge hinged chest lid; origin is hinge")
    # Local Bevy Z=0..0.52 maps Blender Y=0..-0.52.
    m.box((0,-.26,.10),(.72,.52,.20),"plank")
    for x in (-.325,.325):
        m.box((x,-.26,.115),(.045,.55,.23),"bronze" if relic else "iron")
    for y in (-.05,-.47):
        m.box((0,y,.19),(.72,.055,.06),"bronze" if relic else "iron")
    m.box((0,-.50,.09),(.09,.035,.18),"bronze")
    return m

def barrel(name="barrel"):
    m=Model(name,description="Staved oak barrel with metal hoops")
    m.cylinder((0,0,.29),.23,.58,"plank",12,top_radius=.21)
    for z in (.13,.46):
        m.cylinder((0,0,z),.235,.045,"iron",12)
    m.cylinder((0,0,.57),.20,.02,"plank",12)
    return m

def rack(name="firewood_rack"):
    m=Model(name,description="Open timber rack with stacked split firewood")
    for x in (-.58,.58):
        m.box((x,0,.44),(.10,.57,.88),"beam")
    for z in (.14,.57):
        m.box((0,0,z),(1.24,.55,.07),"beam")
    for row in range(3):
        for i in range(5):
            x=-.47+i*.23+(row%2)*.06
            m.cylinder((x,0,.22+row*.14),.074,.47,"beam",7)
    return m

def fence(name,length=6,along_z=False):
    m=Model(name,description="Six-unit split-rail timber fence with pointed hand-hewn posts")
    count=max(3,int(length/1.45)+1)
    for i in range(count):
        q=-length/2+i*length/(count-1)
        x,y=(0,q) if along_z else (q,0)
        m.box((x,y,.40),(.10,.11,.80),"beam")
        m.cylinder((x,y,.86),.067,.18,"beam",5,top_radius=.005)
    for h in (.35,.62):
        if along_z: m.beam((0,-length/2,h),(0,length/2,h),.075,.065,"plank")
        else: m.beam((-length/2,0,h),(length/2,0,h),.075,.065,"plank")
    return m

def well():
    m=Model("well",description="Round stone well with timber winch and tiled canopy")
    for layer in range(3):
        for i in range(12):
            a=i*math.tau/12
            m.box((.54*math.cos(a),.54*math.sin(a),.13+layer*.18),
                  (.25,.20,.17),"fieldstone" if i%3 else "limestone")
    for x in (-.66,.66):
        m.box((x,0,.94),(.12,.12,1.40),"beam")
    m.beam((-.72,0,1.21),(.72,0,1.21),.09,.09,"beam")
    m.cylinder((0,0,1.20),.10,.38,"plank",9)
    roof_gable(m,1.52,1.10,1.65,.36,"clay_roof",.09)
    return m

def lantern():
    m=Model("lantern",description="Iron lantern with glass panes and arched handle")
    m.box((0,0,.045),(.23,.23,.09),"iron")
    for x in (-.10,.10):
        for y in (-.10,.10):
            m.box((x,y,.23),(.025,.025,.37),"iron")
    m.box((0,0,.42),(.25,.25,.045),"iron")
    m.cylinder((0,0,.46),.11,.08,"bronze",8,top_radius=.025)
    m.beam((-.08,0,.46),(0,0,.60),.025,.025,"iron")
    m.beam((0,0,.60),(.08,0,.46),.025,.025,"iron")
    # Live code's flame/light is retained; the glass is a dark translucent-looking shell.
    for x in (-.105,.105): m.box((x,0,.22),(.009,.15,.25),"bronze",(.72,.53,.27,1))
    return m

def brazier(name="brazier"):
    m=Model(name,description="Tripod riveted iron fire bowl with embers left to runtime")
    for i in range(3):
        a=i*math.tau/3
        m.beam((.18*math.cos(a),.18*math.sin(a),.44),
               (.31*math.cos(a),.31*math.sin(a),0),.055,.055,"iron")
    m.cylinder((0,0,.46),.32,.15,"iron",12,top_radius=.38)
    for i in range(8):
        a=i*math.tau/8
        m.box((.34*math.cos(a),.34*math.sin(a),.50),(.055,.055,.055),"bronze")
    return m

def grindstone():
    m=Model("grindstone",description="Treadle grindstone on a timber support frame")
    for x in (-.27,.27): m.box((x,0,.34),(.09,.53,.68),"beam")
    m.cylinder((0,0,.51),.25,.18,"granite",12)
    m.beam((0,-.36,.54),(0,.36,.54),.06,.06,"iron")
    m.box((.13,-.28,.08),(.52,.11,.07),"plank")
    return m

def sign(name="notice_board",height=1.26):
    m=Model(name,description="Carved wooden community notice and parchment postings")
    for x in (-.46,.46):
        m.box((x,0,height*.5),(.09,.10,height),"beam")
    m.box((0,0,height*.79),(1.08,.12,.48),"plank")
    for x,z in ((-.27,.92),(.18,1.04),(.26,.81)):
        m.box((x,-.068,z),(.22,.006,.13),"plaster",(.90,.78,.61,1))
        m.box((x,-.074,z+.03),(.13,.005,.008),"beam")
    m.box((0,0,height+.06),(1.2,.18,.09),"beam")
    return m

def shrine(name="shrine"):
    m=Model(name,description="Wayside stone shrine with carved brass icon and votive slab")
    m.box((0,0,.13),(.82,.60,.26),"fieldstone")
    m.box((0,0,.68),(.57,.39,.84),"limestone")
    m.box((0,-.205,.68),(.33,.025,.45),"fieldstone")
    m.cylinder((0,-.24,.81),.12,.035,"bronze",10)
    m.box((0,-.24,.84),(.05,.025,.27),"bronze")
    m.box((0,-.24,.84),(.20,.025,.05),"bronze")
    m.box((0,0,1.15),(.71,.55,.12),"limestone")
    return m

def ballista():
    m=Model("ballista",description="Timber torsion ballista; bolt fires toward Bevy -Z")
    m.box((0,0,.22),(1.30,1.66,.15),"beam")
    for x in (-.5,.5):
        m.box((x,0,.52),(.12,1.50,.59),"beam")
    m.beam((0,.74,.68),(0,-.76,.67),.17,.14,"plank")
    for sx in (-1,1):
        m.beam((sx*.13,-.47,.70),(sx*.63,.03,.82),.11,.12,"beam")
        m.cylinder((sx*.30,-.30,.68),.16,.16,"bronze",10)
    m.box((0,-.32,.80),(.08,.86,.055),"iron")
    m.box((0,.71,.54),(.25,.20,.22),"iron")
    return m

def training_dummy():
    m=Model("training_dummy",description="Straw training dummy with crossarm and worn leather shield")
    m.cylinder((0,0,.57),.065,1.10,"beam",7)
    m.beam((-.27,0,.77),(.27,0,.77),.055,.05,"beam")
    m.cylinder((0,-.02,.70),.20,.48,"straw",10)
    for z in (.55,.82): m.box((0,-.20,z),(.42,.03,.04),"beam")
    m.cylinder((0,0,1.12),.115,.20,"straw",9)
    return m

def sailboat(name="sailboat_a"):
    m=Model(name,description="Clinker-built river sailboat with ribbed hull and linen sail")
    # Keel base at zero. Waterline is local Bevy Y about 0.28.
    for row,(z,w,d) in enumerate(((.05,.38,1.10),(.19,.66,1.42),(.39,.84,1.56))):
        m.box((0,0,z),(d,w,.09),"plank")
    for x in (-.59,-.32,0,.32,.59):
        m.beam((x,-.37,.38),(x,.37,.38),.07,.07,"beam")
    for side in (-1,1):
        m.beam((-.72,side*.37,.35),(.72,side*.37,.35),.08,.09,"beam")
    m.cylinder((0,0,1.0),.055,1.65,"beam",8,top_radius=.035)
    m.beam((0,0,1.51),(.70,0,1.36),.04,.04,"beam")
    # Sail is two linen triangles, a sculpted split surface rather than a box.
    m.quad((.01,-.018,1.46),(.63,-.018,1.30),(.01,-.018,.59),
           (.01,-.018,.59),"plaster",(.81,.77,.62,1))
    m.box((-.45,0,.46),(.30,.42,.10),"beam")
    return m

def prop_models():
    out=[
        cart(),hay_bale(),rack(),fence("fence_run",6,True),
        well(),lantern(),brazier(),grindstone(),barrel(),
        chest(),chest_lid(),chest("chest_relic_body",True),
        chest_lid("chest_relic_lid",True),ballista(),training_dummy(),sailboat(),
    ]
    # Castle court accents and economic activity.
    out.extend((
        sign(),sign("bounty_board",1.47),
        Model("trough",description="Water trough with plank sides"),
        Model("bench",description="Timber bench"),
        Model("garden",description="Kitchen garden, low stone border and trellis"),
        Model("woodpile",description="Stacked split logs"),
        Model("laundry",description="Laundry line and washing tub"),
        Model("armory",description="Weapon stand and shields"),
        Model("axe_display",description="Axe display on a rack"),
        Model("sword_display",description="Sword display on a rack"),
        Model("tax_booth",description="Town tax booth"),
        shrine(),Model("scaffold",description="Timber scaffold"),
        Model("stone_pile",description="Stack of dressed construction stones"),
        Model("guild_banner",description="Guild pennant on static cross-arm"),
        Model("guild_goods",description="Merchant crate and barrel wares"),
        Model("bell_frame",description="Heavy oak bell frame, dynamic bell kept in code"),
        Model("wood_yard",description="Timber storage yard"),
        Model("hay_corner",description="Hay-stacked courtyard corner"),
        Model("cart_corner",description="Courtyard cart and goods"),
    ))
    by={m.name:m for m in out}
    t=by["trough"];t.box((0,0,.19),(1.05,.48,.12),"plank")
    for x in (-.51,.51): t.box((x,0,.25),(.07,.54,.37),"beam")
    for y in (-.23,.23): t.box((0,y,.25),(1.10,.06,.37),"beam")
    t.box((0,0,.28),(.93,.34,.015),"iron",(.49,.62,.64,1))
    b=by["bench"]
    for x in (-.46,.46):
        for y in (-.17,.17): b.box((x,y,.22),(.08,.08,.44),"beam")
    b.box((0,0,.45),(1.14,.46,.08),"plank")
    b.box((0,.23,.69),(1.14,.07,.49),"plank")
    g=by["garden"];g.box((0,0,.03),(1.48,1.00,.06),"earth")
    for x in (-.75,.75): g.box((x,0,.11),(.09,1.12,.19),"fieldstone")
    for y in (-.50,.50): g.box((0,y,.11),(1.57,.09,.19),"fieldstone")
    for x in (-.46,0,.46):
        for y in (-.29,.29): g.box((x,y,.12),(.18,.24,.15),"wattle",(.39,.67,.31,1))
    w=by["woodpile"]
    for row in range(4):
        for i in range(6-row):
            x=-.55+i*.21+row*.1
            w.cylinder((x,0,.11+row*.16),.085,.55,"beam",7)
    laundry=by["laundry"]
    for x in (-.86,.86): laundry.box((x,0,.61),(.07,.07,1.22),"beam")
    laundry.beam((-.87,0,1.14),(.87,0,1.14),.012,.012,"beam")
    for x in (-.43,.32):
        laundry.box((x,-.02,.83),(.42,.03,.51),"plaster",(.76,.74,.64,1))
    laundry.cylinder((-.65,-.26,.16),.22,.32,"plank",9)
    ar=by["armory"]
    for x in (-.48,.48): ar.box((x,0,.61),(.09,.18,1.22),"beam")
    for z in (.36,.96): ar.box((0,0,z),(1.06,.17,.08),"beam")
    for x in (-.33,0,.33):
        ar.beam((x,-.12,.48),(x,-.12,1.12),.024,.028,"iron")
        ar.box((x,-.12,1.15),(.16,.03,.04),"iron")
    veteran=renamed(ar,"armory_veteran","Veteran armory with iron shields and extra spears")
    for x in (-.37,.37):
        veteran.cylinder((x,-.25,.71),.18,.06,"iron",10)
        veteran.box((x,-.29,.71),(.055,.02,.29),"bronze")
    out.append(veteran)
    for name,blade in (("axe_display",.24),("sword_display",.42)):
        v=by[name]
        v.box((0,0,.47),(.92,.18,.94),"beam")
        for x in (-.26,.26):
            v.beam((x,-.11,.13),(x,-.11,.90),.035,.035,"beam")
            v.box((x,-.13,.83),(.19 if name=="axe_display" else .07,.04,blade),"iron")
    tax=by["tax_booth"]
    tax.box((0,0,.36),(1.24,.95,.72),"plaster")
    tax.box((0,-.50,.62),(.71,.10,.22),"plank")
    tax.box((0,-.60,.79),(.88,.10,.11),"beam")
    roof_gable(tax,1.24,.95,.78,.36,"clay_roof",.09)
    tax.box((.33,-.52,.93),(.18,.04,.18),"bronze")
    sc=by["scaffold"]
    for x in (-.85,.85):
        for y in (-.57,.57): sc.box((x,y,.72),(.10,.10,1.44),"beam")
    sc.box((0,0,1.07),(1.92,1.32,.11),"plank")
    for x in (-.85,.85):
        sc.beam((x,-.57,.15),(x,.57,1.15),.055,.05,"beam")
    stone=by["stone_pile"]
    for row in range(3):
        for i in range(4-row):
            stone.box((-.55+i*.34+row*.16,0,.11+row*.21),
                      (.31,.35,.20),"limestone")
    banner=by["guild_banner"]
    banner.cylinder((0,0,.78),.048,1.56,"beam",8)
    banner.beam((0,0,1.50),(.68,0,1.50),.042,.044,"beam")
    banner.quad((.16,-.014,1.47),(.62,-.014,1.47),(.55,-.014,.88),
                (.16,-.014,.94),"burgundy_cloth")
    goods=by["guild_goods"]
    for x,y,s in ((0,0,.42),(.46,.08,.32),(-.40,.24,.30)):
        goods.box((x,y,s/2),(s,s*.85,s),"plank")
        for z in (.08,s-.06): goods.box((x,y,z),(s+.03,s*.9,.035),"iron")
    goods.cylinder((-.43,-.30,.21),.17,.42,"plank",9)
    bell=by["bell_frame"]
    bell.box((0,0,.09),(2.10,1.35,.18),"fieldstone")
    bell.box((0,0,.26),(1.70,1.00,.16),"limestone")
    for x in (-.75,.75):
        bell.box((x,0,1.34),(.16,.16,2.0),"beam")
        bell.beam((x,0,1.02),(x*.55,0,2.18),.10,.10,"beam")
    bell.beam((-.95,0,2.20),(.95,0,2.20),.16,.18,"beam")
    roof_gable(bell,2.50,1.15,2.34,.45,"slate",over=0)
    yard=by["wood_yard"]
    for x in (-1.07,1.07):
        for y in (-.61,.61): yard.box((x,y,.39),(.10,.10,.78),"beam")
    yard.box((0,0,.14),(2.20,1.40,.07),"plank")
    for row in range(3):
        for j in range(5):
            yard.cylinder((-.72+j*.37,.13,.22+row*.15),.08,.89,"beam",7)
    hay=by["hay_corner"]
    for x,y in ((-.38,0),(.18,-.25),(.25,.22)):
        tmp=hay_bale()
        off=len(hay.vertices)
        hay.vertices += [(vx+x,vy+y,vz) for vx,vy,vz in tmp.vertices]
        hay.uvs += tmp.uvs;hay.colors += tmp.colors
        hay.faces += [tuple(off+i for i in face) for face in tmp.faces]
    corner=by["cart_corner"]
    tmp=cart()
    corner.vertices=tmp.vertices[:];corner.uvs=tmp.uvs[:]
    corner.colors=tmp.colors[:];corner.faces=tmp.faces[:]
    corner.box((-.52,.22,.26),(.41,.36,.52),"plank")
    # Exact meadow identifiers used by the runtime replacement hooks.
    out.extend((
        renamed(cart("meadow_hay_cart"),"meadow_hay_cart"),
        hay_bale("meadow_hay_bale"),
        renamed(rack("meadow_firewood_rack"),"meadow_firewood_rack"),
        fence("meadow_fence",6,True),
        renamed(stump_model(),"meadow_sit_stump"),
    ))
    scare=Model("meadow_scarecrow",description="Straw scarecrow with linen tunic and wide hat")
    scare.cylinder((0,0,.75),.045,1.50,"beam",7)
    scare.beam((-.49,0,1.12),(.49,0,1.12),.045,.05,"beam")
    scare.box((0,0,1.02),(.39,.19,.51),"wattle",(.72,.67,.48,1))
    scare.cylinder((0,0,1.48),.15,.23,"straw",8)
    scare.cylinder((0,0,1.61),.28,.05,"beam",9)
    out.append(scare)
    hive=Model("meadow_beehive",description="Coiled skep beehive on a timber pallet")
    hive.box((0,0,.07),(.57,.57,.13),"beam")
    for j in range(6):
        hive.cylinder((0,0,.20+j*.10),.26*(1-.075*j),.095,"straw",11)
    hive.box((0,-.265,.28),(.09,.02,.045),"iron")
    out.append(hive)
    crate=Model("meadow_crate_stack",description="Handmade rough supply crate stack")
    for x,y,z,s in ((0,0,0,.44),(.38,.08,0,.36),(.13,0,.44,.37)):
        crate.box((x,y,z+s/2),(s,s*.88,s),"plank")
        for xx in (x-s/2,x+s/2):
            crate.box((xx,y,z+s/2),(.045,s*.94,s),"beam")
    out.append(crate)
    fire=Model("meadow_fire_base",description="Stone-ringed camp fire base; flames stay live")
    for i in range(12):
        a=i*math.tau/12
        fire.boulder((.28*math.cos(a),.28*math.sin(a),0),.11,"fieldstone",i)
    for a in (0,math.pi/2):
        fire.beam((-.25*math.cos(a),-.25*math.sin(a),.09),
                  (.25*math.cos(a),.25*math.sin(a),.09),.09,.09,"beam")
    out.append(fire)
    return out

def stump_model():
    from nature import stump
    return stump()

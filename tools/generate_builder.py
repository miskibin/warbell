"""Emit self-contained Three.js HTML that LOFTS each part from its front(width) x
side(depth) profiles (loft_spec.json): sweep an octagonal cross-section scaled by
(halfW[i],halfD[i]) up the height -> real shaped low-poly (helm dome, boot toe, shield
slab). True colours from the sheet, even lighting. Assembled bottom-up by part heights."""
import json, os
OUT=r"D:\tileworld-bevy-forest\model_proportions"
spec=json.load(open(os.path.join(OUT,"loft_spec.json")))

HTML=r"""<!doctype html><html><head><meta charset=utf8><title>Knight (loft)</title>
<style>body{margin:0;background:#202329;overflow:hidden;font:13px monospace;color:#ccc}
#hud{position:fixed;top:8px;left:8px;z-index:9;background:#0009;padding:8px 10px;border-radius:6px;line-height:1.7}
button{background:#333;color:#ddd;border:1px solid #555;border-radius:4px;padding:2px 8px;cursor:pointer;margin-right:4px}</style>
</head><body><div id=hud>
<b>Knight</b> &mdash; loft from front&times;side &middot; drag orbit, scroll zoom<br>
<button onclick="W()">wireframe</button><button onclick="X()">explode</button>
<button onclick="window.spin=!window.spin">spin</button><span id=info></span></div>
<script type="importmap">{"imports":{
"three":"https://unpkg.com/three@0.160.0/build/three.module.js",
"three/addons/":"https://unpkg.com/three@0.160.0/examples/jsm/"}}</script>
<script type="module">
import * as THREE from 'three';
import {OrbitControls} from 'three/addons/controls/OrbitControls.js';
const SPEC=__SPEC__, P=SPEC.parts, SEG=12, BOX=0.62, BULGE=0.45;
const sp=(t,p)=>Math.sign(t)*Math.pow(Math.abs(t),p);   // superellipse term (boxy<1)

function loft(part){
  const hw=part.halfW, hd=part.halfD, bg=part.bulge||[], N=hw.length, h=part.h, v=[], idx=[];
  const ring=i=>{const y=h*(1-i/(N-1)), b=bg[i]||0;
    for(let s=0;s<SEG;s++){const a=s/SEG*Math.PI*2, cs=Math.cos(a), sn=Math.sin(a);
      const X=sp(cs,BOX)*Math.max(hw[i],0.02);
      let Z=sp(sn,BOX)*Math.max(hd[i],0.02);
      if(sn>0) Z += sn*Math.max(hd[i],0.02)*b*BULGE;     // round the front face
      v.push(X,y,Z);}};
  for(let i=0;i<N;i++) ring(i);
  for(let i=0;i<N-1;i++)for(let s=0;s<SEG;s++){
    const a=i*SEG+s,b=i*SEG+(s+1)%SEG,c=(i+1)*SEG+(s+1)%SEG,d=(i+1)*SEG+s;
    idx.push(a,b,d,b,c,d);}
  const top=v.length/3; v.push(0,h,0); const bot=v.length/3; v.push(0,0,0);
  for(let s=0;s<SEG;s++){idx.push(top,(s+1)%SEG,s); idx.push(bot,(N-1)*SEG+s,(N-1)*SEG+(s+1)%SEG);}
  const g=new THREE.BufferGeometry();
  g.setAttribute('position',new THREE.Float32BufferAttribute(v,3));
  g.setIndex(idx); g.computeVertexNormals(); return g;
}
const scene=new THREE.Scene(); scene.background=new THREE.Color(0x202329);
const cam=new THREE.PerspectiveCamera(38,innerWidth/innerHeight,0.1,100); cam.position.set(7,5.2,14);
const r=new THREE.WebGLRenderer({antialias:true}); r.outputColorSpace=THREE.SRGBColorSpace;
r.setSize(innerWidth,innerHeight); r.setPixelRatio(devicePixelRatio); document.body.appendChild(r.domElement);
const ctl=new OrbitControls(cam,r.domElement); ctl.target.set(0,3,0);
scene.add(new THREE.HemisphereLight(0xdfe8ff,0x4a4338,2.0));
const k=new THREE.DirectionalLight(0xfff6ea,2.0); k.position.set(5,9,7); scene.add(k);
const f=new THREE.DirectionalLight(0xcdd8ff,1.3); f.position.set(-5,4,9); scene.add(f);   // front fill
const b2=new THREE.DirectionalLight(0xaab8ff,0.6); b2.position.set(-6,3,-6); scene.add(b2);
scene.add(new THREE.GridHelper(16,16,0x444,0x2c2f36));

const mat=name=>{const c=P[name].rgb; return new THREE.MeshStandardMaterial({
  color:new THREE.Color(`rgb(${c[0]},${c[1]},${c[2]})`),flatShading:true,roughness:0.85,metalness:0.0});};
const G=new THREE.Group();
function add(name,x,yBottom,opt={}){
  const m=new THREE.Mesh(loft(P[name]),mat(name)); m.position.set(x,yBottom,opt.z||0);
  if(opt.rot)m.rotation.set(...opt.rot); m.userData.y0=yBottom; G.add(m); return m;}

// ---- assemble bottom-up (head units, ground=0) ----
const b=P.boot.h*0.55, s=b+P.shin.h*0.92, t=s+P.thigh.h*0.92, hip=t+P.thigh.h*0.0+ P.thigh.h*0.0;
const HIP=t+P.thigh.h*0.85;
const cuiB=HIP-0.12, shoulder=cuiB+P.cuirass.h*0.92;
const legX=0.46, armX=P.cuirass.halfW.reduce((a,c)=>Math.max(a,c),0)+0.18;
for(const sg of [1,-1]){
  add('boot', sg*legX, 0, {});
  add('shin', sg*legX, b);
  add('thigh',sg*legX, s);
  add('pauldron', sg*(armX-0.05), shoulder-P.pauldron.h*0.55);
  add('arm', sg*armX, shoulder-P.pauldron.h*0.35-P.arm.h);
  add('gauntlet', sg*armX, shoulder-P.pauldron.h*0.35-P.arm.h-P.gauntlet.h*0.75);
}
add('tabard',0, HIP-P.tabard.h*0.72);
add('cuirass',0, cuiB);
// neck stub so the helm doesn't sink into the shoulders
const neckGap=0.10, neckH=P.helm.h*0.12+neckGap, neckY=shoulder-0.04;
const neck=new THREE.Mesh(new THREE.CylinderGeometry(0.22,0.26,neckH,10),
  new THREE.MeshStandardMaterial({color:0x2c2f35,flatShading:true,roughness:0.9}));
neck.position.set(0,neckY+neckH/2,0); neck.userData.y0=neck.position.y; G.add(neck);
add('helm',0, neckY+neckH-0.02);
add('shield', -(armX+0.30), cuiB+0.15, {rot:[0,0.18,0]});
add('sword',  (armX+0.40), b, {});
scene.add(G);
document.getElementById('info').textContent=' | '+Object.keys(P).length+' parts';
let wire=false; window.W=()=>{wire=!wire;G.traverse(o=>o.material&&(o.material.wireframe=wire));};
let exp=false; window.X=()=>{exp=!exp;G.children.forEach(m=>m.position.y=m.userData.y0*(exp?1.3:1));};
window.spin=false;
window.view=(p)=>{const d=14;
  if(p=='front')cam.position.set(0,3.4,d);
  else if(p=='side')cam.position.set(d,3.4,0.001);
  else cam.position.set(7,5.2,d);
  ctl.target.set(0,3.4,0); ctl.update();};
addEventListener('keydown',e=>{if(e.key=='1')view('front');if(e.key=='2')view('side');if(e.key=='3')view('persp');});
addEventListener('resize',()=>{cam.aspect=innerWidth/innerHeight;cam.updateProjectionMatrix();r.setSize(innerWidth,innerHeight);});
(function loop(){requestAnimationFrame(loop); if(window.spin)G.rotation.y+=0.01; ctl.update(); r.render(scene,cam);})();
</script></body></html>"""
html=HTML.replace("__SPEC__",json.dumps(spec))
for fn in ("knight.html","index.html"):
    open(os.path.join(r"D:\tileworld-bevy-forest\tools",fn),"w").write(html)
print("wrote tools/knight.html + index.html  (",len(spec["parts"]),"parts )")

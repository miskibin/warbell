"""Publish inspected in-game captures as a portable local comparison gallery."""
import hashlib
import json
from pathlib import Path
import shutil

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "docs/studies/world-2026-10-08"
SCENES = [
    ("castle", "Osada i zamek", "Budynki, mury, dachy, bruk i gospodarstwa.", True),
    ("forest", "Roślinność lasu", "Porównanie podszytu: w obu wersjach są te same nowe drzewa.", True),
    ("path", "Ścieżka przez las", "Ziemia, ściółka, kamienie i roślinność wśród nowych drzew.", True),
    ("village", "Między domami", "Drewno, tynk, kamień i drobne wyposażenie osady.", False),
    ("gameplay", "Widok gracza", "Rzeczywista kamera trzecioosobowa. Bohater czeka na osobny PR.", False),
    ("swamp", "Mokradła", "Trzciny, rośliny bagienne, chaty i ruiny.", False),
    ("snow", "Śnieżne wzgórza", "Śnieg, skały i roślinność chłodnego biomu.", False),
    ("desert", "Pustynia", "Piasek, kaktusy i pustynne budowle.", False),
    ("rocky", "Skaliste wyżyny", "Kamienne podłoże, klify i głazy.", False),
    ("fort", "Fort orków", "Palisada, wieże, wielka hala i obóz.", False),
]


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    evidence = OUT / "evidence"
    evidence.mkdir(exist_ok=True)
    rows = []
    records = []
    for key, title, caption, paired in SCENES:
        for mode in (["world", "native"] if paired else ["world"]):
            actual_mode = "trees" if key == "forest" and mode == "native" else mode
            source = ROOT / f"target/world-preview/final-{key}-{actual_mode}"
            record = json.loads((source / "run.json").read_text(encoding="utf-8"))
            assert record["screenshot_saved"] and not record["errors"] and record["exit_code"] == 0
            shutil.copy2(source / "shot.png", OUT / f"{key}-{mode}.png")
            destination = evidence / source.name
            destination.mkdir(exist_ok=True)
            for name in ("run.json", "game.log"):
                shutil.copy2(source / name, destination / name)
            record["image_sha256"] = hashlib.sha256((source / "shot.png").read_bytes()).hexdigest()
            records.append(record)
        rows.append(dict(key=key, title=title, caption=caption, paired=paired))
    (OUT / "captures.json").write_text(json.dumps(records, indent=2), encoding="utf-8")
    template = r'''<!doctype html>
<html lang="pl"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Warbell · Świat z Blendera</title>
<style>
:root{color-scheme:dark;font-family:system-ui,sans-serif;background:#121713;color:#ede9dc}*{box-sizing:border-box}body{margin:0}header,main,footer{max-width:1440px;margin:auto;padding:24px 30px}header{display:flex;align-items:end;justify-content:space-between;gap:20px}small,.muted{color:#a9b6a7}small{letter-spacing:.15em;text-transform:uppercase}h1{font:500 clamp(26px,4vw,44px) Georgia,serif;margin:8px 0}p{line-height:1.6;margin:6px 0}a{color:#d7c48e}button{background:#222c24;color:inherit;border:1px solid #445044;border-radius:7px;padding:10px 15px;cursor:pointer;font:inherit}button[aria-pressed=true]{background:#d5c697;border-color:#d5c697;color:#162019}nav{display:flex;flex-wrap:wrap;gap:8px;margin-bottom:18px}.stage{position:relative;aspect-ratio:16/9;overflow:hidden;background:#263027;border:1px solid #485047;border-radius:10px}.stage img{position:absolute;width:100%;height:100%;object-fit:contain}.before{clip-path:inset(0 50% 0 0)}.line{position:absolute;left:50%;top:0;bottom:0;width:2px;background:#f0e5bd}.line::after{content:'↔';position:absolute;top:48%;left:-18px;background:#ece0bd;color:#1d2b1f;border-radius:50%;width:38px;height:38px;text-align:center;line-height:38px;font-size:22px}.tag{position:absolute;top:16px;padding:6px 10px;border-radius:4px;background:#152017da;color:#fff;font-size:13px}.tag.old{left:16px}.tag.new{right:16px}.controls{display:flex;align-items:center;gap:15px;margin-top:14px;flex-wrap:wrap}.controls input{flex:1;min-width:140px;accent-color:#d5c697}h2{font:500 27px Georgia,serif;margin:24px 0 8px}.note{border-top:1px solid #394439;margin-top:30px;padding-top:20px;font-size:14px;max-width:900px}footer{color:#a9b6a7;font-size:13px}.hidden{display:none!important}@media(max-width:650px){header,main,footer{padding:18px 14px}header{display:block}.tag{top:8px;font-size:11px}.controls{gap:8px}button{padding:8px 10px}}
</style>
<header><div><small>Warbell / pierwsza iteracja otoczenia</small><h1>Świat z Blendera</h1><p class="muted">Prawdziwe ujęcia z gry · 1920 × 1080 · jakość High</p></div><a href="../trees-2026-10-08/index.html">Wcześniejszy test drzew ↗</a></header>
<main><nav aria-label="Wybierz miejsce" id="scenes"></nav>
<div class="stage"><img id="after" alt="Otoczenie po podmianie modeli"><img id="before" class="before" alt="Oryginalne otoczenie"><span class="tag old" id="oldtag">Przed</span><span class="tag new">Blender</span><div class="line" id="line"></div></div>
<div class="controls"><button id="whole" aria-pressed="true">Pokaż po podmianie</button><button id="compare" aria-pressed="false">Porównaj</button><input id="slider" class="hidden" type="range" min="0" max="100" value="50" aria-label="Granica porównania"><a id="full" target="_blank">Pełna rozdzielczość ↗</a></div>
<h2 id="title"></h2><p id="caption" class="muted"></p>
<div class="note"><p>Modele architektury, skał, roślin i wyposażenia powstały przez Blender MCP, w palecie dopasowanej do nowych drzew. Teren zachowuje geometrię i kolizje gry; nowe tekstury ziemi, ścieżek i bruku pochodzą z Blendera.</p><p class="muted">Postacie oraz dynamiczne efekty pozostają z obecnej gry. To podgląd pierwszej iteracji, uruchamiany opcją <code>FOREST_BLENDERWORLD=1</code>. <a href="README.md">Zakres i sprawdzenie</a> · <a href="captures.json">Metadane ujęć</a></p></div>
</main><footer>Ujęcia wykonane w silniku Bevy. Bez retuszu ani generowania obrazu końcowego.</footer>
<script>
const scenes=__SCENES__;let selected=scenes[0],comparison=false;
const $=id=>document.getElementById(id);
function render(){ $('after').src=selected.key+'-world.png';$('before').src=selected.paired?selected.key+'-native.png':selected.key+'-world.png';$('full').href=$('after').src;$('title').textContent=selected.title;$('caption').textContent=selected.caption;for(const id of ['before','oldtag','line','slider'])$(id).classList.toggle('hidden',!comparison||!selected.paired);$('compare').classList.toggle('hidden',!selected.paired);$('whole').setAttribute('aria-pressed',!comparison);$('compare').setAttribute('aria-pressed',comparison);document.querySelectorAll('nav button').forEach((b,i)=>b.setAttribute('aria-pressed',scenes[i]===selected));}
scenes.forEach(s=>{const b=document.createElement('button');b.textContent=s.title;b.onclick=()=>{selected=s;comparison=false;render()};$('scenes').append(b)});
$('whole').onclick=()=>{comparison=false;render()};$('compare').onclick=()=>{comparison=true;render()};$('slider').oninput=e=>{const v=e.target.value;$('before').style.clipPath=`inset(0 ${100-v}% 0 0)`;$('line').style.left=v+'%'};render();
</script></html>'''
    (OUT / "index.html").write_text(template.replace("__SCENES__", json.dumps(rows, ensure_ascii=False)), encoding="utf-8")
    print(f"Gallery: {OUT / 'index.html'} ({len(records)} verified captures)")


if __name__ == "__main__":
    main()

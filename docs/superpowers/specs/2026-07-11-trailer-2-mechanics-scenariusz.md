# Trailer 2 — „Mechanics" — scenariusz (2026-07-11)

Scenariusz drugiego trailera Warbell, tym razem **prowadzonego przez mechaniki** (nie klimat).
Oparty na researchu kodu: inwentarz mechanik (`src/main.rs` + roadmap), mapa hooków
reżyserskich (`demo.rs` / `capture.rs` / env-hooki `FOREST_*`) oraz porównanie z trailerem #1
(era ~v0.16: explore → build → work → talk → rescue → defend).

## Dlaczego te sceny (wnioski z researchu)

**Świeże od trailera #1** (v0.17–v0.20, lipiec 2026) — to musi być na ekranie:

1. **Walka „wiedźmińska"** — dodge roll, combo chain, parry/riposte, combat stance, hit-FX
   (07-03) + pełny **first-person viewmodel** (07-05→07-10). Zupełnie nowa mechanika — nośnik
   trailera.
2. **Łucznicy / longbow** — draw-and-loose, balistyczne strzały, 1/3 milicji, sentry na keepie.
3. **Overhaul mapy** — MAP_SCALE 2.6, mesa-góry, jezioro z wodospadem, drogi, 5 animowanych
   landmarków, naturalne klify. Stare ujęcia explore są nieaktualne — nowe pokażą inny świat.
4. **Cage rescue po przebudowie** — realni jeńcy w klatce, animowane drzwi, kwestie uwolnionych.

**Nigdy nie pokazane w trailerze #1** (choć istniały): biome wardens (od 07-03 z rzeźbionymi
modelami), **Gnashfang Hold + Warlord** (win condition!), war bell (nazwa gry!), wieże/ballista
z bliska, upgrade tree, succession. Trailer #1 w ogóle nie miał bossów ani celu gry — #2 dostaje
przez to dramaturgię: *dzień → przygotowania → noc → polowanie → finał u Warlorda*.

**Świadomie pomijamy**: rival stronghold w akcji (FOREST_RAID nie ma reżysera/kamery — drogi
nowy mode o niskim priorytecie), animowane upgrade tree (dłubanie w `tree_ui.rs`; statyczny
panel starczy jako insert), succession (slow-mo beat psuje tempo ~90-sekundowego trailera;
kandydat na osobny GIF), chest mimic (brak stagingu).

## Struktura

Długość celowa **~85–95 s** (10 scen + karta tytułowa), 30 fps. Muzyka: `music-bed.ogg` pod
scenami 1–6, crossfade w `soot-banner-dread.ogg` (trim od ~69.5 s!) od sceny 7 do końca.
Tytuły drawtext (EBGaramond) po angielsku. Klipy niosą wyłącznie SFX (muzyka w assemblacji,
`amix … normalize=0`).

| # | Scena | Tytuł na ekranie | Czas | Status produkcyjny |
|---|-------|------------------|------|--------------------|
| 0 | Cold open: dzwon | *(brak — sam dzwon)* | 4 s | ✅ gotowe hooki |
| 1 | Eksploracja nowej mapy | **Explore a Living Island** | 10 s | ✅ gotowe |
| 2 | Timelapse budowy grodu | **Forge a Stronghold** | 8 s | ✅ gotowe |
| 3 | Wioska przy pracy | **Tend the Realm** | 8 s | ✅ gotowe |
| 4 | Salwa łuczników | **Raise an Army** | 8 s | 🔧 nowy mode `volley` (mały) |
| 5 | Walka rycerza (TPS + wstawka FP) | **Master the Blade** | 10 s | 🔧 nowy mode `duel` (średni) |
| 6 | Ratunek jeńców | **Free the Captured** | 7 s | ✅ gotowe |
| 7 | Nocne oblężenie | **Hold the Night** | 12 s | ✅ gotowe |
| 8 | Biome warden | **Hunt the Wardens** | 6 s | 🔶 staging jest, kadr do wypracowania |
| 9 | Wyłom w Gnashfang Hold + Warlord | **Slay the Warlord** | 8 s | 🔶 `FOREST_BREACH` jest, dobrać kamerę |
| — | Karta tytułowa | **WARBELL** + call-to-action | 5 s | ✅ (post, drawtext) |

Razem ~86 s + crossfady (xfade ~0.7 s, offsety kumulatywne — liczy `build_trailer.ps1`).

## Sceny — szczegóły

### 0. Cold open — „The Bell" (4 s)

Czerń → cięcie na dzwon wojenny tłukący o zmierzchu. To jest *nazwa gry* — brand w pierwszej
sekundzie, zero tekstu.

```powershell
$env:FOREST_CLIP="promo/frames/bell"; $env:FOREST_BELLTEST="1"
$env:FOREST_TIME="0.78"   # zmierzch
# kamera blisko dzwonu — BELL_POS ≈ (4.5, 7.5); kadr nisko, lekko pod dzwon:
$env:FOREST_CAM="8,5.5,11,4.5,4.5,7.5"; $env:FOREST_CLIP_FRAMES="140"; cargo run
```

SFX: `war-bell.ogg` zsynchronizowany z zamachem (BELLTEST tłucze w ~12-sek. pętli — warmup tak,
by nagranie startowało tuż przed zamachem; sprawdzić na klatkach próbnych). Bez muzyki przez
pierwsze ~2 s — sam dzwon, potem wchodzi `music-bed`.

### 1. Explore a Living Island (10 s)

Bohater idzie ścieżką przez przebudowany świat — kamera TPS (prawdziwy follow-cam, nie god-cam).
Dwa pod-ujęcia sklejone: (a) marsz `explore` przez las, (b) 4-sek. orbit wokół jednego z
animowanych landmarków lub jeziora z wodospadem (pokazuje skalę overhaul-u mapy).

```powershell
# (a) marsz:
$env:FOREST_CLIP="promo/frames/explore"; $env:FOREST_DEMO="explore"; $env:FOREST_TPS="1"
$env:FOREST_TPS_PITCH="0.6"; $env:FOREST_CLIP_FRAMES="200"; $env:FOREST_CLIP_WARMUP="300"; cargo run
# (b) orbit landmarku (nisko! wysokość ~14, patrz CLAUDE.md o białych god-camach):
$env:FOREST_CLIP="promo/frames/landmark"; $env:FOREST_CLIP_ORBIT="<x>,1.5,<z>,20,14,8"
$env:FOREST_TIME="0.3"; $env:FOREST_CLIP_FRAMES="120"; cargo run
```

SFX: `forest-ambient` bed + ptaki; wodospad, jeśli ujęcie (b) go łapie.

### 2. Forge a Stronghold (8 s)

Timelapse budowy: palisada → bramy → wieże → chaty, 1 element / 24 klatki (17 kroków ≈ 408
klatek przy defaultach — przy 8 s sceny albo przyciąć w moncie do najlepszego fragmentu, albo
nagrać całość i przyspieszyć 2× w ffmpeg; preferowane to drugie, timelapse znosi przyspieszenie).

```powershell
$env:FOREST_CLIP="promo/frames/build"; $env:FOREST_DEMO="build"
$env:FOREST_CLIP_ORBIT="0,2,0,26,15,6"; $env:FOREST_CLIP_FRAMES="440"; cargo run
```

SFX: `castle-ambient` + młotki/kurz (build_fx ma pop-in — na ścieżce dźwiękowej porozkładać
uderzenia co 0.8 s, jak w encoderze trailera #1).

### 3. Tend the Realm (8 s)

Wioska przy pracy: kamera over-the-shoulder śledzi drwala przez pełny cykl rąbanie → drzewo
pada → zniesienie drewna; w tle górnicy z taczkami.

```powershell
$env:FOREST_CLIP="promo/frames/work"; $env:FOREST_DEMO="work"
$env:FOREST_CLIP_WARMUP="2600"   # NIE mniej — drwale muszą dojść i zacząć machać
$env:FOREST_CLIP_FRAMES="240"; cargo run
```

SFX: `chop-wood-{1,2,3}` na kadencji ~2.1 s do momentu upadku drzewa, potem `dog`/`goat`.

### 4. Raise an Army (8 s) — 🔧 nowy mode `volley`

Linia łuczników napina i puszcza salwę — balistyczne strzały lecą łukiem w nadchodzących orków.
Staging już istnieje (`FOREST_ARCHERS=8` + `FOREST_WAVE=1` + `FOREST_TOWN=1`), brakuje tylko
**kamery**: `defend_cam` kadruje melee na dziedzińcu, nie linię strzelecką.

**Do zrobienia:** tryb `volley` w `src/demo.rs` — reuse setupu `defend`, kamera nisko za
plecami łuczników patrząca wzdłuż toru strzał (strzała w kadrze od spuszczenia cięciwy do
trafienia). Wycena z researchu: ~40–70 linii, najtańszy z nowych mode'ów.

SFX: bow-shot (nowe SFX z 07-06) w rytm salw + `ork-grunt` przy trafieniach.

### 5. Master the Blade (10 s) — 🔧 nowy mode `duel`

Serce trailera — nowa walka. Choreografia (frame-locked po `ClipProgress.frame`!):

1. (TPS, ~5 s) Bohater w gold gear kontra 2–3 orki: ork się zamachuje → **dodge roll** w bok →
   kontra **combo chop→slash→thrust** → ostatni ork pada z floating damage number i hit-stopem.
2. (cięcie, FP, ~3 s) To samo starcie od środka: viewmodel miecza+tarczy, jeden blok + riposta.
3. (opcjonalny akcent, ~2 s) **Heavy Strike** z paskiem ładowania albo jedna Weapon Art
   (Ground Slam — shockwave) jako kropka nad i.

**Do zrobienia:** tryb `duel` w `src/demo.rs` — rozszerzenie wzorca `defend_hero` (cykl
zamachów już tam jest) o fazę uniku i pinowanie 2–3 orków; `FOREST_ROLLTEST`/`FOREST_SWINGTEST`
dowodzą, że animacje odpalają się z kodu. Wycena: ~60–90 linii (średni). Wstawkę FP nagrać
osobnym przebiegiem tego samego mode'a z `FOREST_FP=1` + `FOREST_IMMORTAL=1` (żeby succession
beat nie porwał kamery — patrz CLAUDE.md).

```powershell
$env:FOREST_EQUIP="sword_gold,gold_armor"; $env:FOREST_IMMORTAL="1"  # oba przebiegi
```

SFX: `sword-swing`→`sword-hit-{1,2,3}`, `block`, `player-attack-grunt`, `ork-roar`, na Ground
Slam coś basowego (do dobrania przy montażu).

### 6. Free the Captured (7 s)

Gotowy mode `rescue`, po overhaul-u wygląda o klasę lepiej niż w trailerze #1: realni chłopi w
klatce, drzwi się otwierają, jeńcy wychodzą, pada kwestia (napis + VO).

```powershell
$env:FOREST_CLIP="promo/frames/rescue"; $env:FOREST_DEMO="rescue"
$env:FOREST_CLIP_WARMUP="500"; $env:FOREST_CLIP_FRAMES="210"; cargo run
```

SFX: walka krótkim akcentem, skrzyp drzwi klatki, kwestia VO (transkrypty w
`src/audio/lines.rs` — napis musi zgadzać się z klipem).

### 7. Hold the Night (12 s) — kulminacja obronna

Nocne oblężenie w pełnej skali: horda z pochodniami, **wieże + ballista strzelają homing
boltami** (nigdy nie pokazane z bliska!), linia łuczników na murze (`FOREST_ARCHERS`), bohater
trzyma bramę. Orbit battle-cam z mode'u `defend`; `siege_clip_refill` dolewa orków do 36, więc
bitwa nie rzednie.

```powershell
$env:FOREST_CLIP="promo/frames/defend"; $env:FOREST_DEMO="defend"
$env:FOREST_WAVE="1"; $env:FOREST_DEFEND="1"; $env:FOREST_TOWN="1"
$env:FOREST_ARCHERS="6"; $env:FOREST_EQUIP="sword_gold,gold_armor"
$env:FOREST_CLIP_WARMUP="1500"   # niebo musi zdążyć ściemnieć, horda się zebrać
$env:FOREST_CLIP_FRAMES="360"; cargo run
```

**Tu wchodzi muzyka nocna** (`soot-banner-dread`, trim 69.5 s). SFX: `wave-start-roar` na
cięciu, gęsta warstwa ~20 eventów (recepta z trailera #1: ork VO `charge/taunt/death_2/gate`,
`ork-grunt`, pary swing→hit na kadencji bohatera, `block`).

### 8. Hunt the Wardens (6 s) — 🔶

Zmiana rejestru po bitwie: samotny bohater w obcym biomie staje naprzeciw **wardena** (rzeźbione
modele od 07-03; pasywny do pierwszego ciosu — czyli da się podejść i kadrować bez walki).
Kadr: TPS zza pleców bohatera, warden góruje w kontrze; biom inny niż las (pustynia albo śnieg
— kontrast z resztą trailera).

**Do wypracowania:** wardeni patrolują, więc framing bywa loteryjny — najpierw `FOREST_SHOT`-y
zwiadowcze na pozycję wardena w biomie, potem `FOREST_HERO="x,z"` + `FOREST_TPS=1` + krótki
klip. Jeśli okaże się zbyt flaky, plan B: powolny orbit (`FOREST_CLIP_ORBIT` nisko) wokół
patrolującego wardena bez bohatera. Zero nowego kodu, tylko iteracja kadrów.

SFX: sam wiatr/ambient biomu + niski growl — cisza po bitwie działa dramaturgicznie.

### 9. Slay the Warlord (8 s) — finał

`FOREST_BREACH=1` wyłamuje bramę Gnashfang Hold na pierwszej klatce: garnizon się budzi,
**Warlord** wychodzi. Kadr: TPS/niska kamera zza bohatera stojącego w wyłomie, Hold w ogniach
pochodni, Warlord w głębi. NIE choreografujemy walki z bossem (wyceniona na ~100–150 linii —
za drogo, a cliffhanger „boss wstaje" jest lepszym zakończeniem niż wynik starcia).

```powershell
$env:FOREST_CLIP="promo/frames/warlord"; $env:FOREST_BREACH="1"
$env:FOREST_HERO="<x,z wyłomu>"; $env:FOREST_TPS="1"; $env:FOREST_NIGHT="1"
$env:FOREST_CLIP_FRAMES="240"; cargo run
```

**Do wypracowania:** pozycja bohatera/kamery względem bramy Holdu (kilka `FOREST_SHOT`
iteracji). SFX: `orc-march-tallow` motyw bossa, gate-crash, ork VO `gate`, niski werbel.

Cięcie do czerni na szczycie napięcia → **karta tytułowa**: „WARBELL" (EBGaramond, duży),
pod spodem „Build by day. Hold the night." + „Play free on itch.io", ostatnie uderzenie dzwonu
(`war-bell.ogg`) jako stinger. Klamra z cold openem.

## Plan produkcji

Kolejność realizacji (najpierw to, co odblokowuje resztę):

1. **Kod:** mode `volley` (mały) i `duel` (średni) w `src/demo.rs` — jedyne nowe rzeczy do
   napisania. Reguły: aktorzy ruszają dopiero gdy `ClipProgress.recording`, beaty po
   `ClipProgress.frame / fps` (nigdy `time.elapsed_secs()`), systemy simowe z
   `run_if(in_state(Modal::None))`, kamera/napisy w `PostUpdate`.
2. **Zwiad kadrów** (`FOREST_SHOT`): pozycja wardena (scena 8), wyłom Holdu (scena 9), wybór
   landmarku (scena 1b), kadr dzwonu (scena 0).
3. **Nagrania scen gotowych**: 0, 1, 2, 3, 6, 7 — od razu, niezależnie od punktu 1.
4. **Nagrania scen nowych**: 4, 5 po wejściu mode'ów.
5. **Encode per scena** (SFX only, `amix … normalize=0`, `alimiter=limit=0.9`) → **assemblacja**
   (`build_trailer.ps1`: xfade + drawtext + muzyka) → **volumedetect ostatniej sceny**
   (mean −10…−14 dB, inaczej muzyka utonęła) → ekstrakcja klatek kontrolnych → publikacja
   (release + GIF-y 480px/15fps na itch.io: dzwon, duel, salwa łuczników, siege).

Przypomnienia z bolesnych lekcji (skill `trailer-maker` + CLAUDE.md): warmupy hojne (work 2600,
defend 1500 — inaczej scena startuje „pusta" albo za jasna); **czytać klatki próbne przed
enkodą 400 sztuk**; grep logów za `Screenshot saved`/`panic`/`Validation` zanim zaufamy PNG;
`soot-banner-dread` jest cichy na początku — zawsze trim od ~69.5 s.

## Poza zakresem (świadomie)

- **Walka z Warlordem** — najdroższy mode; cliffhanger robi tę robotę taniej.
- **Rival stronghold w akcji** — `FOREST_RAID` bez reżysera; materiał na trailer #3 / devlog.
- **Succession, upgrade tree w akcji, chest mimic, fishing** — za wolne/menu-owe na trailer;
  succession i mimik to dobre kandydaty na osobne devlogowe GIF-y.
- **Panel-inserty UI** (`FOREST_PANEL=tree/inv`) — trzymamy w zanadrzu: jeśli scena 5 wyjdzie
  krótsza, 1.5-sek. flash War Table między sceną 5 a 6 z podpisem „Grow Stronger".

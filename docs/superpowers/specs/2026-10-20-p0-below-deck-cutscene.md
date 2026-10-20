# P.0 „Na pokładzie” — cutscenka T2 (rig wideo) + brakujące assety

Scenariusz: `2026-10-13-kampania-scenariusz-design.md` §P.0, punkty 1–2 (czerń + tytuł, kajuta,
dialog Harrow/Wat). Część grywalna (punkt 3: zadania giermka, Maud pod pokładem, hamak) **nie** wchodzi
w tę cutscenkę.

Kod: `src/scenes/cabin.rs` (`SceneId::ShipCabin`), dźwięk: `tools/cutscene/cabin_audio.py` →
`assets/cutscene/ship_cabin.cues`. Tak jak P.3 to **rig do nagrania filmu**, nie funkcja gry — brak
triggera, skipa i zapisu.

```bash
# still jednej chwili (sekunda sceny):
BEVY_ASSET_ROOT=$PWD FOREST_SCENE=cabin FOREST_SCENE_T=20 FOREST_SHOT=/tmp/cabin.png ./target/debug/tileworld_bevy_forest
# cały film (49.5 s @ 24 fps) + ścieżka dźwiękowa:
BEVY_ASSET_ROOT=$PWD FOREST_SCENE=cabin FOREST_CLIP=target/clips/cabin FOREST_CLIP_FPS=24 \
  FOREST_CLIP_FRAMES=1188 FOREST_CLIP_WARMUP=90 FOREST_CLIP_AUDIO=0 ./target/debug/tileworld_bevy_forest
ffmpeg -framerate 24 -i target/clips/cabin/frame_%05d.png -c:v libx264 -pix_fmt yuv420p /tmp/cabin_v.mp4
python3 tools/cutscene/cabin_audio.py mix /tmp/cabin_v.mp4 cabin.mp4
```

## Jak jest zbudowana

- Kajuta stoi na otwartym morzu ~150 u od Wreck Cove (`worldmap::wreck_cove_axis`), rufą od wyspy.
  Cały plan (kajuta, aktorzy, światła, kamera) jedzie na jednej sztywnej ramie statku
  (`cabin::ship_xf`: przechył 7.4 s, kiwanie, falowanie) — w kadrze kajuta stoi, a ruszają się morze
  za oknami rufowymi, lampa (wahadło przeciw przechyłowi) i deszcz.
- Noc (niebo przypięte na 0.82), światło: lampa na haku (PointLight z cieniami, migotanie), świeca na
  stole, zimny księżyc przez okna, cztery błyskawice (najmocniejsza na „Before it burned?”).
- Obsada: Harrow na krześle (`Sitting_Idle_Loop` / `Sitting_Talking_Loop`, dopasowanie miednicy do
  siedziska), Wat klęczy przy jego lewej nodze (`Fixing_Kneeling`), głowy prowadzone skryptem
  (Harrow: w dół na pasek → w dal → w okno → z powrotem na Wata; Wat podnosi wzrok na „No, Ser.”
  i na ostatnią kwestię). Usta z obwiedni nagranych kwestii (lip-sync jak w P.3).
- Hełm na stole i tarcza na burcie za krzesłem to **prawdziwe części zbroi rycerza z gry**
  (`player::spawn_part_prop`) — ta sama zbroja, którą Wat założy na plaży.
- Ujęcia (thin lens, ostrość na mówiącym): A szeroki z lampą → B nisko zza ramienia Wata na Harrowa
  („perspektywa giermka”) → C dłonie na pasku → C2 twarz Wata → D Harrow z hełmem → E profil w oknie
  (piorun) → F z góry na Wata → G1 profil → G2 najazd z kąta giermka (tarcza za głową) → H1 pasek
  dopięty → H2 szeroki w czerń.
- Napisy: wszystkie kwestie z nagrań `a1_harrow_01…04`, `a1_wat_01…02`, cięte na pauzach.

## Brakujące assety (scena jest zbudowana tak, jakby były — po wrzuceniu wchodzą same)

Każdy slot jest szukany najpierw jako prawdziwy asset; dopóki go nie ma, stoi blockout / zastępstwo.

### Modele (kit GLB, `tools/blender_forest/` → nazwy w kicie)

| slot | co | teraz |
|---|---|---|
| `cs_cabin` | kajuta rufowa: podłoga z desek, belki pokładu, burty z wręgami, gródź z drzwiami, ściana rufowa z 3 oknami (szprosy, ołów, otwarte światła), ława pod oknami. Podłoga y=0, +Z = rufa, wnętrze ~4.2 × 3.4 u, belki na 1.72 u | blockout |
| `cs_cabin_table` | ciężki stół na kozłach, blat na 0.6 u | blockout |
| `cs_cabin_chair` | krzesło z poręczami (siedzisko 0.42 u, przodem do +Z) | blockout |
| `cs_cabin_hammock` | hamak Wata przy lewej burcie (potrzebny też w części grywalnej) | blockout |
| `cs_cabin_lantern` | lampa olejowa na łańcuchu, punkt zaczepienia w originie, wisi w −Y (~0.5 u); płomień/szybki emisyjne | blockout + świecąca kostka |
| (opcjonalnie) list do Reeve z pieczęcią, mapa, kubek, kałamarz | rekwizyty na stole — dziś jeden scalony blockout | blockout |

Skrzynia (`chest_wood_*`) i skrzynka (`wooden_crate_01`) są już w kicie i są użyte.

### Postacie (`npc_skin::LOOKS`)

| look | co | teraz |
|---|---|---|
| `harrow` | Ser Godric Harrow: ~50 lat, zmęczona twarz (jedyna twarz w prologu w kadrze od razu), siwiejąca broda, **pełna zbroja płytowa jak rycerz gracza, bez hełmu** (hełm leży na stole) | zastępstwo: `guard` (najwyższy w obsadzie) — w kolczudze, nie w płytach |
| `wat` | Wat, giermek, ~40 lat, kaftan, twarz widoczna (gracz zna ją od P.0) | zastępstwo: `villager_m` |

Wystarczy dodać look o tej nazwie do `LOOKS` — `cabin::look()` sam go podmieni.

### Animacje

- **zapinanie paska nagolennika** (klęczenie, dłonie przy łydce siedzącego, pociągnięcie i dopięcie
  sprzączki) — dziś ogólne `Fixing_Kneeling`, dłonie nie trafiają w nagolennik;
- (opcjonalnie) `Sitting_Idle` z ręką na poręczy / zwrot tułowia do okna — dziś obrót robi tylko głowa.

### Muzyka

- `assets/audio/cutscene/below_deck.ogg` — **A1-1 „Below Deck”** (prompt w
  `docs/audio/cutscene-music-suno.md`), wchodzi na „Do you know what they said about me in the city?”
  (~-28 dB, ściszana pod głosem), wychodzi z obrazem.

### SFX (`assets/audio/sfx/cutscene/`, stereo, -18 LUFS — dopisane w `docs/audio/sfx-brakujace.md`)

| klucz | gdzie |
|---|---|
| `cs_strap_cinch` | pasek przeciągany przez sprzączkę (10.6 / 12.2 / 13.4 s) i dociągnięty (42.3 s) |
| `cs_armour_shift` | zbroja Harrowa przy obrocie do okna i z powrotem (25.3 / 36.0 s) |
| `cs_thunder_far` | grzmot ~1.5 s po każdym błysku |
| `cs_wave_slam` | fala w burtę (na czerni, 22.3 s, 43.9 s) |

Pętla `cs_ship_cabin` (skrzypienie, deszcz, lampa) już jest i gra pod całą sceną.

### Efekty (opcjonalne, poprawią obraz)

- krople / strugi deszczu na szybach okien rufowych (dziś deszcz to smugi w powietrzu za oknami);
- piorun widoczny w oknie (dziś tylko błysk światła + rozjaśnienie deszczu);
- piana / wyższa fala za rufą (morze z gry jest spokojne jak na sztorm).

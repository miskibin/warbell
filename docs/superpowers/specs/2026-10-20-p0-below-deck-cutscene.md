# P.0 „Na pokładzie” — cutscenka T2 (rig wideo) + brakujące assety

Scenariusz: `2026-10-13-kampania-scenariusz-design.md` §P.0, punkty 1–2 (czerń + tytuł, kajuta,
dialog Harrow/Wat). Część grywalna (punkt 3: zadania giermka, Maud pod pokładem, hamak) **nie** wchodzi
w tę cutscenkę — ale cutscenka kończy się tam, gdzie ona się zaczyna (Wat wychodzi z listem, niżej).

Kod: `src/scenes/cabin.rs` (`SceneId::ShipCabin`), dźwięk: `tools/cutscene/cabin_audio.py` →
`assets/cutscene/ship_cabin.cues`. Tak jak P.3 to **rig do nagrania filmu**, nie funkcja gry — brak
triggera, skipa i zapisu.

```bash
# still jednej chwili (sekunda sceny):
BEVY_ASSET_ROOT=$PWD FOREST_SCENE=cabin FOREST_SCENE_T=40 FOREST_SHOT=/tmp/cabin.png ./target/debug/tileworld_bevy_forest
# cały film (86 s @ 24 fps) + ścieżka dźwiękowa:
BEVY_ASSET_ROOT=$PWD FOREST_SCENE=cabin FOREST_CLIP=target/clips/cabin FOREST_CLIP_FPS=24 \
  FOREST_CLIP_FRAMES=2064 FOREST_CLIP_WARMUP=90 FOREST_CLIP_AUDIO=0 ./target/debug/tileworld_bevy_forest
ffmpeg -framerate 24 -i target/clips/cabin/frame_%05d.png -c:v libx264 -pix_fmt yuv420p /tmp/cabin_v.mp4
python3 tools/cutscene/cabin_audio.py mix /tmp/cabin_v.mp4 cabin.mp4
```

## Tekst do nagrania

Mówione kwestie i tagi ElevenLabs: `docs/audio/elevenlabs-p0-poklad.md`.
Muzyka, która nie leci pod dialogiem: `docs/audio/cutscene-music-suno.md` (A1-1).
Poniższy montaż opisuje film ze starymi klipami. `cabin.rs` nie ruszać, dopóki nie ma nowych take'ów.

## Reżyseria (86 s, trzy ruchy)

Pierwsza wersja (49.5 s) mówiła cały dialog scenariusza jednym ciągiem, przy jednej nieruchomej
pozie — trzy osobne tematy (pasek → „Forty peasants” → miasto) bez niczego pomiędzy, a Harrow rzucał
zdania z niczego. Teraz każdą kwestię coś w kajucie **wywołuje**, a między ruchami jest akcja i cisza:

1. **Pasek** (4.6–26 s). Długo nikt nic nie mówi: lampa, deszcz, Wat przy nagolenniku. Sprzączka nie
   siada → „Tighter.” → Wat ciągnie → „You've done that strap every day for fifteen years and it's
   still wrong.” → „Yes, Ser.” → wysiłek, sprzączka dopięta: „Hnn. There.”
2. **List** (26–46.6 s). Harrow patrzy na stół (hełm, zapieczętowany list): *(westchnienie)* „The
   letter. To the Reeve. Today, if the sea allows.” Wat wstaje, obchodzi lampę, bierze list ze stołu,
   idzie do drzwi — i **staje**: spod pokładu, przez deski, słychać osadników śpiewających hymn.
   Harrow też słucha, wzrok w podłogę: „Forty peasants and a Reeve. That's what the Crown sends a man
   after the eastern campaign.” (powód kwestii: ten śpiew to cała jego „armia”).
3. **Miasto** (46.6–86 s). Harrow odwraca się do okna, za szybą błyska: „Do you know what they said
   about me in the city?” — pauza, wielki piorun — „Before it burned?” Wat odwraca się od drzwi:
   „No, Ser.” Harrow odwraca się do niego: „Neither do I.” … „Nobody looks at the man behind the
   shield, Wat.” … „That's the point of you.” Wat stoi, wzrok w dół, nie odchodzi; Harrow, z powrotem
   do okna: „Stop hovering.” Drzwi, ciemny korytarz, hymn głośniej przez otwarte drzwi; Harrow zostaje
   sam z lampą → czerń.

**Pożyczone kwestie.** Trzy nagrania pochodzą z części interaktywnej P.0 (`docs/audio/elevenlabs-akt-1.md`):
`a1_harrow_07` „The letter. To the Reeve…”, `a1_harrow_08` „Stop hovering.”, `a1_wat_05` „Hnn. There.”.
Konsekwencja dla części grywalnej: cutscenka kończy się z listem w ręku Wata za drzwiami kajuty, więc
grywalna część naturalnie **zaczyna** się od zadania „zanieść list Reeve” (Maud pod pokładem — ci sami
śpiewający osadnicy), a naramiennik / hełm na skrzynię idą po powrocie. Jeśli kolejność zadań ma zostać
jak w scenariuszu, trzeba nagrać osobną kwestię wysyłającą Wata z kajuty.

Ujęcia (thin lens, ostrość na mówiącym / na Wacie w ruchu):

| s | ujęcie |
|---|---|
| 4.6–10 | A szeroki od drzwi: lampa, Wat przy nodze, okna w deszczu |
| 10–12 · 13.2–14.8 | C dłonie na pasku |
| 12–13.2 | B nisko zza ramienia Wata na Harrowa — „Tighter.” |
| 14.8–20.4 | B2 najazd z kąta giermka, tarcza za głową |
| 20.4–23.4 | C2 twarz Wata — „Yes, Ser.” |
| 23.4–26 | H1 sprzączka dopięta — „Hnn. There.” |
| 26–29.4 | D Harrow z prawej, wzrok na stół — „The letter…” |
| 29.4–33.6 | W1 szeroki z rufowego narożnika: Wat wstaje, do stołu, bierze list |
| 33.6–38.8 | W2 od okien: Wat do drzwi, staje (hymn) |
| 38.8–46.6 | D2 Harrow z miejsca Wata, najazd — „Forty peasants…” |
| 46.6–49.4 | insert: sztorm w oknie rufowym |
| 49.4–55.4 | E twarz w oknie, piorun — „Do you know… / Before it burned?” |
| 55.4–58.6 | F Wat przy drzwiach zza ramienia Harrowa — „No, Ser.” |
| 58.6–62.4 | G1 oko Wata na Harrowa — „Neither do I.” |
| 62.4–66.4 | G2 zza prawego ramienia Wata — „Nobody looks…” |
| 66.4–70.6 | H Wat z bliska, najazd — „That's the point of you.” |
| 70.6–74.2 | I Harrow w oknie — „Stop hovering.” |
| 74.2–78.6 | J zza Wata: drzwi, ciemny korytarz |
| 78.6–86 | K Harrow sam, od drzwi (klamra z A), w czerń |

## Jak jest zbudowana

- Kajuta stoi na otwartym morzu ~150 u od Wreck Cove (`worldmap::wreck_cove_axis`), rufą od wyspy.
  Cały plan (kajuta, aktorzy, światła, kamera) jedzie na jednej sztywnej ramie statku
  (`cabin::ship_xf`: przechył 7.4 s, kiwanie, falowanie) — w kadrze kajuta stoi, a ruszają się morze
  za oknami rufowymi, lampa (wahadło przeciw przechyłowi) i deszcz.
- Noc (niebo przypięte na 0.82), światło: lampa na haku (PointLight z cieniami, migotanie), świeca na
  stole, świeca w kinkiecie przy drzwiach (bez niej Wat przy drzwiach ginie w czerni — lampa jest
  ~2 m dalej), zimny księżyc przez okna, pięć błyskawic (najmocniejsza tuż przed „Before it
  burned?”). Na końcu korytarza za drzwiami ciepłe światło z ładowni (z cieniami: za zamkniętymi
  drzwiami widać tylko szparę pod nimi); Wat odchodzi pod to światło jako sylwetka.
- Lampa wisi dokładnie na linii drzwi → Harrow, więc ujęcia „od Wata” (D2, G1) stoją obok tej linii,
  nie na niej (inaczej lampa zasłania twarz).
- Obsada: Harrow na krześle (`Sitting_Idle_Loop` / `Sitting_Talking_Loop`, dopasowanie miednicy do
  siedziska), głowa prowadzona skryptem (`harrow_beat`: pasek → stół → za Watem → podłoga, hymn →
  okno → Wat → okno). Wat (`wat_track`): klęczy (`Fixing_Kneeling` przytrzymany w przyklęku przez
  `npc_skin::ClipSeek` — scena przewija klip po swoim zegarze), wstaje resztą tego klipu, chodzi
  (`Walk_Loop`, tempo z prędkości), bierze list (`PickUp_Table` przewijany), stoi (`Idle_Loop`),
  obroty wygładzone. List to osobny rekwizyt: leży na stole, od chwyconej klatki jedzie w prawej
  dłoni Wata (`letter_follow`). Drzwi na zawiasie otwierają się na korytarz (`door_open`).
  Usta: wizemy MakeHuman na skórach (`add_face_morphs.py`: szczęka, 14 kształtów, mruganie, zęby, język,
  ciemne wnętrze), grane po ścieżce Rhubarba danej kwestii w czasie sceny (`lipsync::Speech`); twarz Harrowa
  przy oknie ma zimne światło księżyca przez okno (`FaceMoon`, ×7 przy błysku).
- Hełm na stole i tarcza na burcie za krzesłem to **prawdziwe części zbroi rycerza z gry**
  (`player::spawn_part_prop`) — ta sama zbroja, którą Wat założy na plaży.
- Napisy: wszystkie kwestie, cięte na pauzach nagrań (`cabin.rs` `VO` / `LINES`).
- Blockouty nie są gołymi kolorami: każda część dostaje skan CC0 z `assets/forest_pbr`
  (`s_timber` drewno, `s_iron` żelazo, `s_cloth` hamak/liny) z UV rzutowanymi po osiach w układzie
  statku (`cabin::skin_part`; słoje wzdłuż dłuższego boku, deska/belka mieści się w jednej desce
  skanu), kolor wierzchołka zostaje jako odcień. Prawdziwy `cs_cabin` w kicie i tak je zastąpi.
- Obiektyw: DoF thin-lens jest wymuszony na czas filmu także na presecie Low (wstawiany `Dof` +
  `DepthPrepass`, oddawane po scenie), w kajucie szersza przysłona (`lens = band·1.5`, do 9 px);
  grade „taśma”: mocne ziarno, winieta, przygaszone kolory, przyciśnięte czernie, halacja.
- Dźwięk: hymn osadników to dzienny hymn z gry (`audio/day-hymn-1.ogg`) puszczony w miksie przez
  dolnoprzepustowy filtr (flaga `lowpass=` w cue'ach — „przez pokład”), drugi raz jaśniej, gdy drzwi
  są otwarte; kroki Wata to `audio/footsteps/wood-*`. Muzyka, hymn i pętla kajuty są ściszane pod
  głosem.

## Brakujące assety (scena jest zbudowana tak, jakby były — po wrzuceniu wchodzą same)

Każdy slot jest szukany najpierw jako prawdziwy asset; dopóki go nie ma, stoi blockout / zastępstwo.

### Modele (kit GLB, `tools/blender_forest/` → nazwy w kicie)

| slot | co | teraz |
|---|---|---|
| `cs_cabin` | kajuta rufowa: podłoga z desek, belki pokładu, burty z wręgami, gródź z **otworem** na drzwi (x −1.35…−0.65, h 1.46) i kawałkiem ciemnego korytarza za nią, ściana rufowa z 3 oknami (szprosy, ołów, otwarte światła), ława pod oknami. Podłoga y=0, +Z = rufa, wnętrze ~4.2 × 3.4 u, belki na 1.72 u | blockout |
| `cs_cabin_door` | skrzydło drzwi 0.7 × 1.46 u, origin na zawiasie (lewa ościeżnica), skrzydło biegnie w +X; okucia, kółko klamki | blockout |
| `cs_cabin_table` | ciężki stół na kozłach, blat na 0.6 u | blockout |
| `cs_cabin_chair` | krzesło z poręczami (siedzisko 0.42 u, przodem do +Z) | blockout |
| `cs_cabin_hammock` | hamak Wata przy lewej burcie (potrzebny też w części grywalnej) | blockout |
| `cs_cabin_lantern` | lampa olejowa na łańcuchu, punkt zaczepienia w originie, wisi w −Y (~0.5 u); płomień/szybki emisyjne | blockout + świecąca kostka |
| (opcjonalnie) list do Reeve z pieczęcią, mapa, kubek, kałamarz | rekwizyty na stole (list jest osobnym obiektem, bo przechodzi do ręki) | blockout |

Skrzynia (`chest_wood_*`) i skrzynka (`wooden_crate_01`) są już w kicie i są użyte.

### Postacie (`npc_skin::LOOKS`)

| look | co | teraz |
|---|---|---|
| `harrow` | Ser Godric Harrow: ~50 lat, zmęczona twarz (jedyna twarz w prologu w kadrze od razu), siwiejąca broda, **pełna zbroja płytowa jak rycerz gracza, bez hełmu** (hełm leży na stole) | zastępstwo: `guard` (najwyższy w obsadzie) — w kolczudze, nie w płytach |
| `wat` | Wat, giermek, ~40 lat, kaftan, twarz widoczna (gracz zna ją od P.0) | zastępstwo: `archer` (inna twarz niż `guard`) |

Wystarczy dodać look o tej nazwie do `LOOKS` — `cabin::look()` sam go podmieni.

### Animacje

- **zapinanie paska nagolennika** (klęczenie, dłonie przy łydce siedzącego, pociągnięcie i dopięcie
  sprzączki) — dziś ogólne `Fixing_Kneeling` przewijane w przyklęku, dłonie nie trafiają w nagolennik;
- **sięgnięcie po list ze stołu jedną ręką** — dziś `PickUp_Table` (podnoszenie oburącz, krótkie);
- **otwarcie drzwi** (ręka na klamce, pchnięcie) — dziś drzwi otwierają się same, Wat w nie wchodzi;
- (opcjonalnie) `Sitting_Idle` z ręką na poręczy / zwrot tułowia do okna — dziś obrót robi tylko głowa.

### Muzyka

- `assets/audio/cutscene/below_deck.ogg` — **A1-1 „Below Deck”** (prompt w
  `docs/audio/cutscene-music-suno.md`), wchodzi, gdy Harrow odwraca się do okna (46.6 s, ~-28 dB,
  ściszana pod głosem), wychodzi z obrazem. Hymn pod pokładem jest (pożyczony dzienny hymn z gry);
  docelowo można nagrać osobny — śpiew osadników a cappella / z liry korbowej, z daleka.

### SFX (`assets/audio/sfx/cutscene/`, stereo, -18 LUFS — dopisane w `docs/audio/sfx-brakujace.md`)

| klucz | gdzie |
|---|---|
| `cs_strap_cinch` | pasek: szarpnięcie (10.4 s), dociąganie po „Tighter.” (13.4 s), dopięty na „Hnn.” (24.0 s) |
| `cs_armour_shift` | zbroja Harrowa przy obrotach głowy/tułowia (26.1 / 46.7 / 58.9 / 69.7 s) |
| `cs_thunder_far` | grzmot ~1.5 s po każdym błysku |
| `cs_wave_slam` | fala w burtę (na czerni, 47.1 s, 79.3 s) |
| `cs_paper_rustle` | list zdjęty ze stołu (32.65 s) |
| `cs_door_creak` | zasuwka i zawiasy drzwi kajuty (74.4 s) |

Pętla `cs_ship_cabin` (skrzypienie, deszcz, lampa) już jest i gra pod całą sceną; kroki po deskach
(`audio/footsteps/wood-*`) też są.

### Efekty (opcjonalne, poprawią obraz)

- krople / strugi deszczu na szybach okien rufowych (dziś deszcz to smugi w powietrzu za oknami);
- piorun widoczny w oknie (dziś tylko błysk światła + rozjaśnienie deszczu);
- piana / wyższa fala za rufą (morze z gry jest spokojne jak na sztorm);
- zejście do ładowni na końcu korytarza (luk + drabina) — dziś jest tam samo światło.

### Tekstury (opcjonalne — dziś blockouty noszą skany z gry)

- deski pokładowe / poszycie statku ze smołowanymi szparami (np. Poly Haven `wood_planks_dirt`,
  `old_planks_02`) — `s_timber` to deski z wiejskiej szopy, nie okrętowe;
- ciemne, natłuszczone drewno mebli (blat stołu z plamami, wytarte poręcze);
- płótno żaglowe na hamak (dziś juta `s_cloth`).

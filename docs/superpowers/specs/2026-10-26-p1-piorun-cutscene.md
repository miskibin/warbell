# P.1 „Piorun” — cutscenka T2 (rig wideo)

Scenariusz: `2026-10-13-kampania-scenariusz-design.md` §P.1. Kod: `src/scenes/storm.rs`
(`SceneId::Storm`, `FOREST_SCENE=storm`), dźwięk: `tools/cutscene/storm_mix.py`. Jak P.0 to **rig do
nagrania filmu**, nie funkcja gry: brak triggera, skipa i zapisu.

```bash
cargo build --release
# film (47.5 s @ 24 fps); FOREST_TPS=1 bo w koda gra rycerz (hero), warm-up = próba wszystkich ujęć
BEVY_ASSET_ROOT=$PWD FOREST_SCENE=storm FOREST_TPS=1 FOREST_CLIP=target/clips/storm FOREST_CLIP_FPS=24 \
  FOREST_CLIP_FRAMES=1140 FOREST_CLIP_WARMUP=260 FOREST_CLIP_AUDIO=0 FOREST_ANIMBUDGET=0 FOREST_ANIMLOD=0 \
  FOREST_SKINSHADOW=all ./target/release/tileworld_bevy_forest.exe
ffmpeg -framerate 24 -i target/clips/storm/frame_%05d.png -c:v libx264 -crf 17 -pix_fmt yuv420p storm_v.mp4
python tools/cutscene/storm_mix.py target/clips/storm_out storm_v.mp4 p1_storm.mp4
```

Look-dev: `FOREST_SCENE_SHEET="t1,t2,…"` (+`FOREST_SCENE_SHEET_HOLD=n`) pod `FOREST_CLIP` zapisuje po
jednej klatce na każdą sekundę sceny (wszystkie ujęcia w jednym bootcie, ~30 s);
`FOREST_STORM_DAY=1` = ta sama inscenizacja w południe bez burzy (widać, gdzie kto stoi);
`FOREST_FLASH_LUX`, `FOREST_STORM_SKY`, `FOREST_STORM_FOG` = siła błysku, kopuły nieba, mgły.

## Montaż (47.5 s, zero dialogu poza kodą, zero muzyki)

| s | obraz | dźwięk |
|---|---|---|
| 0–4 | czerń | kajuta, potem rafa: trzask kadłuba, grzmot, dzwon, krzyki; Wat budzi się (`p1_wat_01`) |
| 4–7.7 | **pokład** wraku przechylony (kamera z rollem): załoga przy dzwonie, rafa, piorun nad morzem | dzwon, sztorm na pokładzie, fala przez burtę |
| 7.7–12 | czerń | kadłub, druga fala; dźwięk schodzi „pod wodę”, sekunda ciszy |
| 12–20.6 | **plaża** z oczu Wata na kolanach: sześć orków z pochodniami wychodzi z lasu, idą, drugi błysk — rozpędzają się do szarży (tempo przechodzi płynnie, `smooth_track`), trzeci błysk: ork w obiektywie, cięcie | przybój, deszcz, kaszel (`p1_wat_02`), warczenie z daleka, okrzyk prowadzącego, okrzyki bandy i tupot coraz bliżej (take Azrak `ork-cry-*`, przestrajany), ryk w obiektywie — szybkie wyciszenie |
| 22–26.9 | **Harrow** na wznak, topór, ork nad nim, Maud patrzy na Wata (podwójny błysk) | grzmot, potem tylko deszcz i pochodnia |
| 26.9–31 | długa czerń | deszcz, oddech Wata |
| 31–37.7 | **hełm** w piasku przy pochodni; za nim, nieostro, Maud z naramiennikiem przy klęczącym Wacie; drugi błysk przenosi ostrość na nich | płyta (`cs_plate_wet`), echo „Tighter.” (P.0), wysiłek (`p1_wat_03`) |
| 37.7–40 | czerń | daleki róg orków |
| 40–46 | **koda**: rycerz w zbroi Harrowa na plaży dobywa miecza i tarczy, wychodzi orkom naprzeciw, napina Ciężki cios (`player::combat::StagedStrike`) i puszcza go na pierwszego — błysk, cięcie | Maud: „Ser Godric's up! Behind him, all of you!” (`a1_maud_04`), okrzyki bandy, ryk, cios w tarczę |

## Jak jest zbudowana

- Plener to prawdziwa Wreck Cove (`wreck_cove.rs`): wyrzucona na plażę karaka gra statek na rafie
  (pokład przechylony, resztę przechyłu robi roll kamery), plaża i las są z gry. `Stage` = układ
  plaży obok dziobu (`inl` w głąb lądu, `along` wzdłuż plaży).
- Błyskawica to **księżyc**: na uderzenie światło kierunkowe obraca się na piorun danego ujęcia i
  rozbłyska (`light()` → `farm_camera`), do tego kopuła chmur wokół kamery (wewnątrz nocnego nieba,
  zasłania gwiazdy i księżyc), jej poświata w miejscu pioruna i sam piorun (wstęga emisyjna).
  Mgła zlewa się z horyzontem kopuły. Deszcz: dwa bloki smug wokół oka, rozjaśniane błyskiem.
- Orkowie to prawdziwi orkowie z gry; nosiciele pochodni wybrani ziarnem (`Armory::spawn`).
- Aktorzy i rekwizyty są tylko w swoim ujęciu (`staged()` ustawia następne ujęcie już w czerni).
- **Próba w warm-upie** (`scenes::rehearsal`): zegar przeskakuje po wszystkich ujęciach, żeby każda
  skóra/ork/światło się narysowało i skompilowało pipeline przed nagraniem. Bez tego ludzie znikali
  w pierwszych klatkach ujęcia (rzucali cień, a widać było same twarze i dłonie).
- W burzy znikają świetliki, cząstki pogody biomu i iskry z pochodni.

## Brakujące / zastępcze

- Głosy i SFX do nagrania (miks podbiera je sam, gdy się pojawią): `docs/audio/elevenlabs-p1-piorun.md`.
- `FOREST_SCENE_FROM=<s>` startuje nagranie od danej sekundy filmu (iteracja jednego fragmentu).
- Deszcz na otwartym i sztorm na morzu są syntezowane w miksie (szum + fale); `cs_beach_storm`,
  `cs_deck_storm`, `cs_thunder_close` z listy brakujących wciąż by pomogły (grzmoty to dwa take'i z P.0).
- Harrow/Wat to dalej zastępstwa (`guard` / `archer`), jak w P.0.

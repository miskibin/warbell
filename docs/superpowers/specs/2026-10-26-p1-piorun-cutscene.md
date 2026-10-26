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

## Montaż (59.6 s @ 24 fps, kod: `storm.rs::at`, `STAND_BEATS`, `FIGHT`, `ORK_SWINGS` / `ORK_DEATHS`)

| s | obraz | dźwięk |
|---|---|---|
| 0–4 | czerń | kajuta; marynarz przez pokład: „Reef! Reef off the bow!”; rafa, grzmot, dzwon, krzyki; Wat budzi się |
| 4–7.7 | **pokład** przechylony, załoga przy dzwonie, piorun nad morzem | dzwon, „Ring it! All hands, brace!” (głos Juliana = Harrow), sztorm |
| 7.7–12 | czerń | fala, wszystko schodzi pod wodę i wypływa w plażę (bez dziury ciszy) |
| 12–20.6 | **plaża** z oczu Wata: orkowie z pochodniami wychodzą z lasu, idą, rozpędzają się w szarżę; Harrow wchodzi w kadr przed Wata z mieczem | daleka banda, warczenie, okrzyk prowadzącego, okrzyki bandy, rozmyty tupot |
| 21.6–28 | **ostatnia walka Harrowa**, sylwetki na tle morza w błyskach: zabija pierwszego, blok drugiego, trzeci trafia go z boku, drugi dobija, Harrow pada | świsty, trafienia, chrząknięcia orków, uderzenie w tarczę, upadek |
| 28.6–33.5 | **Harrow** martwy na piasku, ork nad nim, Maud patrzy na Wata | szept Wata: „Ser...? Ser Godric.”, deszcz |
| 33.5–37.6 | czerń | oddech Wata |
| 37.6–44.3 | **hełm** w piasku przy pochodni; Maud z naramiennikiem przy klęczącym Wacie | Maud: „He's gone, Wat. Put it on. They need a knight standing.”, płyta, wysiłek |
| 44.3–46.6 | czerń | daleka banda: idą znowu |
| 46.6–58.4 | **koda**: rycerz dobywa broni, wychodzi naprzeciw; z boku: blok, seria (cięcie, cięcie, pchnięcie), ork pada, następny i następny; od drugiego trupa kamera wznosi się i oddala nad walką, ciała na piasku; ściemnienie | Maud: „Ser Godric's up! Behind him, all of you!”, okrzyki, tarcza, ciosy, śmierci |

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

# ElevenLabs / SFX — P.1 „Piorun”

Do cutscenki `FOREST_SCENE=storm` (`src/scenes/storm.rs`, miks `tools/cutscene/storm_mix.py`). Miks sam
podbiera każdy plik, gdy pojawi się pod podaną ścieżką; dopóki go nie ma, scena idzie bez niego.
Głosy: Eleven v3, te same co w banku (`assets/audio/vo/campaign/VOICES.md`), każdy blok osobna generacja.

## Głosy (`assets/audio/vo/campaign/`)

| plik | głos | tekst | kiedy |
|---|---|---|---|
| ✅ `p1_sailor_01.ogg` | Wilf | `[shouting, panicked] Reef! Reef off the bow!` | czerń, tuż przed uderzeniem (przez pokład) |
| ✅ `p1_sailor_02.ogg` | Julian (nagrane głosem Harrowa — rozkaz na pokładzie brzmi jak on) | `[shouting over the storm] Ring it! All hands, brace!` | ujęcie pokładu |
| ✅ `p1_wat_04.ogg` | Sully | `[whispering, shaken] Ser...? Ser Godric.` | Harrow na piasku |
| ✅ `p1_maud_01.ogg` | MoanElisa | `[low, urgent] He's gone, Wat. Put it on. They need a knight standing.` | hełm (zastępuje echo „Tighter.”) |
| ❌ `p1_ork_03.ogg` | Azrak (odrzucony: kiczowate; orkowie to dziś SFX `ork-voice-*`) | `[roaring] Graaagh! [pause] [war cry] Hrah! Hrah! [pause] [snarling] Kill! [pause] [roaring] Waaagh!` | dodatkowe gardła bandy |

Już są: `p1_wat_01..03` (Sully: oddech, kaszel, wysiłek), `a1_maud_04` (koda). Głos orków to trzy take'i SFX
(„Aggressive fantasy orc”, „Deep menacing orc growl”, „Fantasy orc warrior”) pocięte na
`assets/audio/ork-voice-{a1,a2,a3,d1,d2,w1,w2}.ogg` — bank orków w grze (`audio/sfx.rs`) i w filmie, gdzie
każdy okrzyk dostaje własne gardło (`storm_mix.py::ork`: wysokość, dubel niżej, saturacja, pokój, dystans).

## SFX (`assets/audio/sfx/cutscene/`, stereo, ~-18 LUFS)

| plik | prompt | długość |
|---|---|---|
| `cs_hull_reef.ogg` | a wooden sailing ship smashing onto a rock reef at night, deep splintering crunch of timbers, the whole hull groaning, heard from inside the ship | 4 s |
| `cs_wave_deck.ogg` | a huge wave crashing over a wooden ship's deck at night, water slamming and pouring across planks, close | 3 s |
| `cs_sword_draw.ogg` | a steel longsword drawn fast from a leather scabbard, metallic ring, close, heavy rain around | 1.5 s |
| `cs_shield_clash.ogg` | a heavy steel sword blow slamming into an orc's iron-rimmed wooden shield, crunching impact and splinters, close, no voices | 1.5 s |

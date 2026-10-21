# Brakujące SFX

Zbiorcza lista odgłosów do wygenerowania. Gotowe pliki: `assets/audio/sfx/life/<klucz>.ogg` (mono 44.1 kHz, ~-18 LUFS, peak ≤ -1 dBFS — konwersję robię ja, wystarczy wrzucić wav/mp3).
Dopisuj tu każdy nowy brakujący odgłos; po dostarczeniu zmień status na ✅.

| status | klucz | prompt | długość |
|---|---|---|---|
| ⬜ | `life_sfx_coins` | a few copper coins dropped and slid across a wooden market counter, close | 1.5 s |
| ⬜ | `life_sfx_purse` | small leather coin purse jingling as it is tied shut | 1.2 s |
| ✅ | `life_sfx_bench_sit` | person sitting down heavily on an old wooden bench, wood creak | 1.0 s |
| ⬜ | `life_sfx_bench_stand` | person getting up from a creaky wooden bench, cloth rustle | 1.0 s |
| ⬜ | `life_sfx_stool` | wooden stool scraping on packed dirt floor | 0.8 s |
| ⬜ | `life_sfx_bread` | tearing a crusty loaf of bread by hand | 1.0 s |
| ⬜ | `life_sfx_mug` | wooden tankard set down on a wooden table, liquid slosh | 0.8 s |
| ⬜ | `life_sfx_pour` | pouring ale from a clay jug into a wooden cup | 2.0 s |
| ⬜ | `life_sfx_well` | medieval well winch cranking with rope creak, then bucket splash far below | 4.0 s |
| ⬜ | `life_sfx_bucket` | carrying a full wooden bucket of water, sloshing, footsteps on dirt | 2.5 s |
| ⬜ | `life_sfx_sweep` | straw broom sweeping a dirt doorstep, slow strokes | 3.0 s, loop |
| ⬜ | `life_sfx_laundry` | shaking out a wet linen sheet and pinning it on a line, flap | 2.0 s |
| ⬜ | `life_sfx_rug_beat` | beating a hanging rug with a wicker beater, three thumps | 2.0 s |
| ✅ | `life_sfx_hammer` | hammering a nail into timber, four strikes | 2.0 s |
| ⬜ | `life_sfx_saw` | hand saw cutting a wooden plank, steady strokes | 3.0 s, loop |
| ⬜ | `life_sfx_whittle` | knife whittling a small piece of wood, soft scrapes | 2.5 s, loop |
| ⬜ | `life_sfx_grain` | handful of grain scattered on dirt, chickens clucking and pecking | 3.0 s |
| ⬜ | `life_sfx_sack` | heavy grain sack thrown onto a shoulder, thump and cloth | 1.0 s |
| ⬜ | `life_sfx_spar` | two wooden practice swords clacking in a slow drill, shuffling boots on packed earth, a grunt | 4.0 s, loop |
| ⬜ | `life_sfx_basket` | wicker basket set down on the ground, creak | 0.8 s |
| ⬜ | `life_sfx_door` | heavy medieval wooden cottage door opening on iron hinges | 1.5 s |
| ⬜ | `life_sfx_door_shut` | heavy wooden cottage door closing with a latch | 1.2 s |
| ⬜ | `life_sfx_shutters` | wooden window shutters swung closed and barred | 1.5 s |
| ⬜ | `life_sfx_cart` | wooden hand cart rolling slowly on a dirt track, wheel creak | 4.0 s, loop |
| ⬜ | `life_sfx_dice` | bone dice rolled on a wooden table, men murmur | 2.0 s |
| ✅ | `life_sfx_chatter` | small group of medieval villagers chatting and laughing quietly outdoors, no clear words | 8.0 s, loop |
| ⬜ | `life_sfx_market` | busy small medieval village market, distant haggling, livestock, no clear words | 10.0 s, loop |
| ⬜ | `life_sfx_hen` | single hen clucking nearby | 1.5 s |
| ✅ | `cs_ship_cabin` | inside a wooden sailing ship's cabin at night in a storm, constant timber creaking and groaning, heavy rain drumming on canvas overhead, muffled waves slapping the hull, an oil lantern squeaking as it swings on its hook, no music, no voices | 20 s, loop |
| ⬜ | `cs_wreck_strike` | wooden sailing ship hits a reef in a violent night storm: sharp close thunder crack, hull timbers splintering and groaning, a ship's bell clanging wildly, distant panicked shouting with no clear words, a heavy wave crashing over the deck | 8 s |
| ⬜ | `cs_strap_cinch` | close-up: a leather strap pulled tight through an iron buckle on a steel greave, creak of leather, small metal clink, quiet cabin | 1.0 s |
| ⬜ | `cs_armour_shift` | a seated man in full plate armour turning in a wooden chair, soft steel plates sliding and clinking, leather creak, close | 1.5 s |
| ⬜ | `cs_thunder_far` | distant thunder rolling a few kilometres away over the sea at night, heard from inside a wooden ship, no rain | 5 s |
| ⬜ | `cs_wave_slam` | heavy wave slamming against a wooden ship's hull heard from inside the cabin, deep thud, timbers groan, water rushing off | 3 s |
| ⬜ | `cs_paper_rustle` | a folded, wax-sealed parchment letter picked up off a wooden table, dry paper rustle, close, quiet room | 0.8 s |
| ⬜ | `cs_door_creak` | an old wooden ship's cabin door: iron latch lifted, then the door swung slowly open on creaking iron hinges | 2.0 s |

## Uwagi
- `cs_strap_cinch`, `cs_armour_shift`, `cs_thunder_far`, `cs_wave_slam`, `cs_paper_rustle`, `cs_door_creak` — cutscenka P.0 „Below Deck” (`tools/cutscene/cabin_audio.py`): **stereo**, -18 LUFS, w `assets/audio/sfx/cutscene/`; cue'y już są w `ship_cabin.cues`, plik wchodzi sam.
- `life_sfx_bench_sit` — Firefly, variation 1 → `life_sfx_bench_sit.ogg`, variation 3 → `life_sfx_bench_sit_2.ogg` (gra losuje; każdy klucz może mieć warianty `_2`…`_4`).
- `life_sfx_hammer` — Firefly, „hammering a nail into timber, four strikes, variation 2” (1.0 s).
- Pętle (loop) muszą się bezszwowo zapętlać — poproś generator o „seamless loop” albo daj 2× dłuższy plik, przytnę sam.
- `life_sfx_chatter`: ElevenLabs, 14.9 s → pętla 13.9 s (crossfade 1 s), mono, -18.8 LUFS. Jeszcze nie w `SFX_KEYS` (`village_life.rs`).
- `cs_ship_cabin`: ElevenLabs, 8 s zamiast 20 s → pętla 6.5 s (crossfade 1.5 s), **stereo** (do filmu), -18 LUFS, w `assets/audio/sfx/cutscene/`. Krótka pętla: przy 90-sekundowej scenie warto kiedyś dograć dłuższą.

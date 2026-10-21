# ElevenLabs: Prolog + Akt I (cutscenki, ogniska, barki)

Kwestie z `docs/superpowers/specs/2026-10-13-kampania-scenariusz-design.md` (P.0–P.3, Rozdz. 1–4, bank T1
Aktu I) + barki misji dopisane 2026-10-14. **Nagrane 2026-10-14 i pocięte:** 130 klipów w
`assets/audio/vo/campaign/a1_<postać>_NN.ogg` (spis głosów: `assets/audio/vo/campaign/VOICES.md`).
Tekst w tabelach to prompt ElevenLabs (tagi v3 w nawiasach kwadratowych). Te tagi nie są napisami: przed
wyświetleniem jako napis trzeba je usunąć.

Retake kajuty P.0 (inny tekst, nowe klucze `p0_*`, starych `a1_harrow_01`–`04` nie nadpisuje):
`docs/audio/elevenlabs-p0-poklad.md`.

## Użyte głosy — NIE ZMIENIAĆ przy dogrywkach

| Postać | Głos ElevenLabs (biblioteka) | Ustawienia (z nazwy pliku) |
|---|---|---|
| Harrow | **Julian** — Deep, Rich and Mature | wspólne nagranie (plik „Sal”): speed 1.05, stability 0.70, similarity 0.70, v3 |
| Wat | **Sully** — Mature, Deep and Intriguing | j.w. |
| Pip | **Omar J** — Very Young Storyteller | j.w. |
| Settler | **Wilf** — Cheerful, Quirky & Northern | j.w. |
| Farmer | **Darby Skelm** — Witty and Sarcastic | j.w. |
| Hamid | **Sal** — calm, Confidential | j.w. |
| Maud | **MoanElisa** — Sultry, Dark, Velvet | speed 0.90, stability 0.50, similarity 0.75, v4 |
| Ork / Ork Shaman | **Dante** — Growly and Menacing Monster | j.w. (brzmi trochę zbyt ludzko, zaakceptowane) |

## Brakuje (do dogrania tym samym głosem)

| klucz | głos | tekst | dlaczego |
|---|---|---|---|
| `a1_wat_32` | Sully | [heavy breathing, exhausted] | generator pominął (sam tag) |
| `a1_pip_15` | Omar J | [coughing weakly] | generator pominął (sam tag) |
| `a1_shaman_01` | Dante | [low guttural ritual chanting] | generator pominął (sam tag) |
| `a1_shaman_02` | Dante | [rasping incantation, rising] | generator pominął (sam tag) |
| `a1_wat_33` | Sully | [hoarse, polite] You're welcome. | cutscenka z farmy (`scenes.rs`), nie było w nagraniu |
| `a1_wat_34` | Sully | [hoarse, dry, after a beat] I know. | j.w. |
| `a1_farmer_06` | Darby Skelm | [flat, unimpressed] Wasn't thanking you. | j.w. (pierwsza kwestia farmera w tej scenie to stare `vo/npc/screaming.ogg`, głos Victor — deep) |
| `a1_maud_47` | MoanElisa | [deadpan] He laid three stones. Then he supervised. | gag murarza (`scenes.rs`), nie było w nagraniu |

Słabsze, ale zostawione: `a1_hamid_09` (zamiast „slow, sad tune” jest krótkie „mm-hmm”, 1.6 s) i
`a1_hamid_10` (samo westchnienie, 0.6 s).

## Klucze → tekst

### HARROW — Julian

| klucz | tekst |
|---|---|
| `a1_harrow_01` | [bored] Tighter. [pause] You've done that strap every day for fifteen years and it's still wrong. |
| `a1_harrow_02` | [bitter] Forty peasants and a Reeve. That's what the Crown sends a man after the eastern campaign. |
| `a1_harrow_03` | [distant] Do you know what they said about me in the city? Before it burned? |
| `a1_harrow_04` | [dry] Neither do I. [pause] Nobody looks at the man behind the shield, Wat. That's the point of you. |
| `a1_harrow_05` | [irritated] Left. It's always the left. |
| `a1_harrow_06` | [impatient] The helmet goes on the chest, Wat. Not near it. On it. |
| `a1_harrow_07` | [sighs] The letter. To the Reeve. Today, if the sea allows. |
| `a1_harrow_08` | [annoyed] Stop hovering. |
| `a1_harrow_09` | [tired] Sleep, Wat. I'll want you at first light, and I'll want you awake. |

### WAT — Sully

| klucz | tekst |
|---|---|
| `a1_wat_01` | [quiet, obedient] Yes, Ser. |
| `a1_wat_02` | [quiet] No, Ser. |
| `a1_wat_03` | [neutral] His squire. |
| `a1_wat_04` | [flat] He's resting. |
| `a1_wat_05` | [effort] Hnn. [exhales] There. |
| `a1_wat_06` | [under his breath] Left. Always the left. |
| `a1_wat_07` | [hoarse, dry] Ser Godric doesn't do churches. |
| `a1_wat_08` | [hoarse] Ser Godric said hold the line. |
| `a1_wat_09` | [hoarse, low] Ser Godric can't climb the tower. |
| `a1_wat_10` | [hoarse] You're the Pasha's man. |
| `a1_wat_11` | [hoarse, curious] What's the song? |
| `a1_wat_12` | [hoarse, flat] Ser Godric is not a carpenter. |
| `a1_wat_13` | [hoarse] Ser Godric wants a wall. Ser Godric will settle for half. |
| `a1_wat_14` | [hoarse, dry] Stones. Ser Godric carries them now. Don't tell anyone. |
| `a1_wat_15` | [hoarse, muttering] The wall won't build itself. Ser Godric checked. |
| `a1_wat_16` | [hoarse, low] Dusk. To the walls. |
| `a1_wat_17` | [hoarse, tired] Dawn. Count the heads. |
| `a1_wat_18` | [hoarse] Torch-bearers. Keep them off the fields. |
| `a1_wat_19` | [hoarse, dry] Barn. Ser Godric said barn. |
| `a1_wat_20` | [hoarse] Fields need hands. Ser Godric has two. Both busy. |
| `a1_wat_21` | [hoarse, low] Shamans in the marsh. Ser Godric goes. Alone is faster. |
| `a1_wat_22` | [hoarse, flat] Camp's burning. Their problem now. |
| `a1_wat_23` | [hoarse] Ser Godric needs men. Brave ones. Stupid will do. |
| `a1_wat_24` | [hoarse] Cages. Somebody's still alive in them. |
| `a1_wat_25` | [hoarse, curt] Out. Walk. Fort's that way. |
| `a1_wat_26` | [hoarse, pained] Ser Godric has had worse. [pause] Ser Godric has never had worse. |
| `a1_wat_27` | [quietly, to himself] Fifteen years behind him. Now they're all behind me. Wrong way round. |
| `a1_wat_28` | [under his breath] Breathe. Rasp. Short words. You're him. |
| `a1_wat_29` | [whispering] Don't take it off. Not out here. |
| `a1_wat_30` | [tired exhale] Nobody looks at the man behind the shield. [pause] They're all looking now. |
| `a1_wat_31` | [pained grunt] |

### PIP — Omar J

| klucz | tekst |
|---|---|
| `a1_pip_01` | [breathless] Is it true? About the axe? |
| `a1_pip_02` | [sulky] He doesn't talk. |
| `a1_pip_03` | [curious] Where's the bell? |
| `a1_pip_04` | [curious] Who's the first lot? |
| `a1_pip_05` | [excited, out of breath] There's a well! [short pause] [amazed, a little breathless] And that... [sad breath] that was a chapel! [short pause] [gasps, excited] There's a bell! Up there! [pause] [softly, in awe] It's whole. [long pause] [eager, hopeful] Can I— *(one take, Omar J; the fort cutscene cuts it at the pauses — `a1_pip_06` is no longer used there)* |
| `a1_pip_06` | [eager] Can I— |
| `a1_pip_07` | [breathless, proud] Did you hear her? That's mine. She's the only thing on this island that does what it's told. |
| `a1_pip_08` | [weak, feverish] Don't let anyone else ring her. |
| `a1_pip_09` | [sulking] The well's clean. I did the well. Nobody says anything about the well. |
| `a1_pip_10` | [eager] It's getting dark. Can I go up? |
| `a1_pip_11` | [earnest] I counted the arrows. There aren't enough. I counted twice. |
| `a1_pip_12` | [earnest] Your sword's dirty, Ser Godric. I can clean it. I won't touch the helmet. |
| `a1_pip_13` | [quiet] I don't like the drums. Do you like the drums? |
| `a1_pip_14` | [weak but cheerful] I'm better. Did anyone ring her? [pause] Good. |

### SETTLER — Wilf

| klucz | tekst |
|---|---|
| `a1_settler_01` | [incredulous] A bell? We haven't got a roof. |
| `a1_settler_02` | [overjoyed] Not a one! Not a single one! You broke their ritual, Ser Godric. That's what it was. That's what brought them. Burn the shamans, the rest go home. |
| `a1_settler_03` | [lowered voice, curious] Does he sleep in it? |
| `a1_settler_04` | [confident] Another camp burned, another quiet night coming. You'll see. |
| `a1_settler_05` | [cheerful] Burn the shamans, sleep like a baby. That's my religion now. |
| `a1_settler_06` | [grumbling] Quiet night, he says. Tell that to my roof. |
| `a1_settler_07` | [awed whisper] Took an axe to the face and still walks. I'd still be lying down. |
| `a1_settler_08` | [nervous] Drums again. Means they're close. Or bored. Either's bad. |

### FARMER — Darby Skelm

| klucz | tekst |
|---|---|
| `a1_farmer_01` | [flat] You said hold the line. Line's fine. Barn's gone. |
| `a1_farmer_02` | [dry] Ser Godric should've said hold the barn. |
| `a1_farmer_03` | [shouting] They're at the barley! Somebody! Anybody in armour! |
| `a1_farmer_04` | [bitter] Third field this week. I'm farming ash now. |
| `a1_farmer_05` | [grudging] Fields held. [pause] Don't let it go to your helmet. |

### HAMID — Sal

| klucz | tekst |
|---|---|
| `a1_hamid_01` | [weary, calling out] You. In the tin. Yes, you. Open this and I'll owe you, which I hate. |
| `a1_hamid_02` | [hoarse, tired] Three weeks. They fed me. Badly, but they fed me. You don't feed what you mean to eat. |
| `a1_hamid_03` | [proud] I'm the Pasha's caravan. The man is in the desert, and he's going to ask me what I was worth. [pause] ...Ser Godric Harrow. He'll want to hear that too. |
| `a1_hamid_04` | [tired] Head. Three weeks of their drums, knight, you'd have a head too. |
| `a1_hamid_05` | [quietly] Theirs. They sing in the evenings. [pause] Not what you'd expect. Sad. Like somebody's leaving for work. |
| `a1_hamid_06` | [wincing] Not the yard, not at sundown. My head. I'll sit inside. |
| `a1_hamid_07` | [bargaining] I'll pay for the bed. Eventually. With interest. Somebody's interest. |
| `a1_hamid_08` | [dry] A well, three walls and a knight who doesn't talk. In the desert we call that a bargain. |
| `a1_hamid_09` | [humming a slow, sad tune] |
| `a1_hamid_10` | [humming softly] [sighs] |

### MAUD — MoanElisa

| klucz | tekst |
|---|---|
| `a1_maud_01` | [unimpressed] You're his boy. |
| `a1_maud_02` | [dry] Same thing, from down here. Does he ever come out? |
| `a1_maud_03` | [flat] Resting. [louder] The knight's resting. Sleep well, everyone, we're guarded by a man who's resting. |
| `a1_maud_04` | [shouting] Ser Godric's up! Behind him, all of you! |
| `a1_maud_05` | [loud, steady] The sea took the squire. Wat. Some of you knew him. |
| `a1_maud_06` | [steady] Ser Godric took an axe on the beach. Throat and face. He'll speak when he's ready and not before. Nobody argues with a man who lived through that. |
| `a1_maud_07` | [reading aloud] "Make for the old fort of Saint Ottilie. Restore it. Ring the King's bell at every dusk." [lowers the letter] That's the whole order. The rest is who to thank. |
| `a1_maud_08` | [dry] Then we'll ring it from the ground. |
| `a1_maud_09` | [matter-of-fact] This one's "for the Lord Commissioner's eyes only." Ser Godric keeps it. Ser Godric's eyes, since the blow. I'll read him what he needs. |
| `a1_maud_10` | [dry] Ask him. |
| `a1_maud_11` | [dry] Then that's your answer, isn't it. |
| `a1_maud_12` | [dry] In the fort, if the first lot didn't melt it for spoons. |
| `a1_maud_13` | [deadpan] People who had a roof. |
| `a1_maud_14` | [weary] The well's whole too. Start with the well. |
| `a1_maud_15` | [firm] At dusk. King's orders, one of them. |
| `a1_maud_16` | [sighs] God help us. A well, a bell, and a wall and a half. |
| `a1_maud_17` | [quietly] Go on. They're watching Ser Godric, not me. |
| `a1_maud_18` | [deadpan] Three walls, no gate, a bell. We're a church with ambitions. |
| `a1_maud_19` | [dry] Ser Godric doesn't do stairs, I've noticed. [lower] Mind the shoulder strap. It's riding up. |
| `a1_maud_20` | [deadpan] Write it down. Next time he'll say barn. |
| `a1_maud_21` | [firm] Swamp fever. It's one night. Someone else can pull a rope. |
| `a1_maud_22` | [sharp, low] Ser Godric can't climb the tower in front of forty people. Different thing. |
| `a1_maud_23` | [long pause] ...Fine. One night. The King can write me a letter. |
| `a1_maud_24` | [hesitant] ...Could be. [flat] Dig. |
| `a1_maud_25` | [dry] Ser Godric inspected the wall. The wall is inspected. The wall is also still a hole. |
| `a1_maud_26` | [dry] Would you take it off, with what's out there? |
| `a1_maud_27` | [dry] Ser Godric isn't a lot of things. Hold the plank. |
| `a1_maud_28` | [calling out] Stones to the north wall! Ser Godric's watching. Look busy. |
| `a1_maud_29` | [dry] Wood, stone, hands. We've got hands. |
| `a1_maud_30` | [dry] The keep's still mostly sky. Stone, Ser Godric. Stone. |
| `a1_maud_31` | [flat] Food's low. Nobody fights well hungry. Or at all. |
| `a1_maud_32` | [calling out] Dusk! Shutters, fires down, everyone on the wall! |
| `a1_maud_33` | [tired] Dawn. Count yourselves. [pause] Good. Same number. |
| `a1_maud_34` | [quiet] Dawn. [pause] Fewer than last night. Bury them before noon. |
| `a1_maud_35` | [urgent] Torches in the fields! Ser Godric, the barley! |
| `a1_maud_36` | [dry] Barley's ash. We'll eat the lecture instead. |
| `a1_maud_37` | [dry] Fields need a fence. Or a knight. Fence is cheaper. |
| `a1_maud_38` | [to the crowd, dry] Ser Godric's off to the marsh. Nobody follow him. He likes it quiet. |
| `a1_maud_39` | [calling out] Anyone who can hold a spear, the yard! Anyone who can't, hold one anyway. |
| `a1_maud_40` | [dry] Twelve farmers and a grudge. Ser Godric, your army. |
| `a1_maud_41` | [warm, brief] You brought them back. [louder] Ser Godric brought them back! |
| `a1_maud_42` | [suspicious] A southerner. Put him by the fire. Not near the grain. |
| `a1_maud_43` | [shouting] Wall's breached! Hold the gap! |
| `a1_maud_44` | [shouting] Ser Godric's down! Get him behind the line! Nobody touch the helmet! |
| `a1_maud_45` | [urgent whisper] Up. They're watching you. |
| `a1_maud_46` | [low, urgent] You're bleeding through the plate. Inside. Now. |

### ORK — Dante

| klucz | tekst |
|---|---|
| `a1_ork_01` | [guttural roar] |
| `a1_ork_02` | [snarling] Iron voice calls! |
| `a1_ork_03` | [growling] Answer! Answer! |
| `a1_ork_04` | [guttural chanting] Hrah. Hrah. Hrah. |
| `a1_ork_05` | [snarling] Tin man! |
| `a1_ork_06` | [dying groan] |

### SHAMAN — Dante

| klucz | tekst |
|---|---|
| `a1_shaman_03` | [furious shriek] |

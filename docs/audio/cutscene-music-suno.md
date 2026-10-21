# Cutscene underscore — Suno prompts (2026-10-13)

Pod `docs/superpowers/specs/2026-10-13-kampania-scenariusz-design.md` (v2, pod master `2026-10-10-kampania-fabula-design.md`). All instrumental, lyrics field = `[instrumental]`.
Subtle dialogue underscore only; action/battle cues are a separate pass.

| # | Name | Scenes | Suno style |
|---|---|---|---|
| 1 | Wreck Cove | P.1 Sztorm (po dzwonie okrętowym), P.2 Rozkazy | sparse medieval ambient underscore, solo bowed nyckelharpa and low cello drone, distant sea wash, no percussion, slow 60 bpm, melancholic but calm, lots of silence between phrases, cinematic film score, instrumental, no vocals |
| 2 | The Governor's Desk | P.3 Fort, Rozdz. 1–4 ogniska, Rozdz. 9 ognisko Wat–Maud | subtle medieval chamber underscore, soft lute arpeggio and quiet viola, occasional harpsichord note, dry and understated, slightly wry mood, slow 70 bpm, minimal, Kingdom Come Deliverance style, dialogue background, instrumental |
| 3 | Dusk Bell | Rozdz. 1 Pierwszy dzwon, Rozdz. 3 cicha noc (sam dron, bez taiko), Rozdz. 12 Wieża | tense quiet medieval underscore, single low church-bell-like drone, bowed bass, soft taiko heartbeat very far away, rising unease, slow 55 bpm, dark ambient cinematic, instrumental, no melody, no vocals |
| 4 | Three Stalls and a Goat | Rozdz. 2, 4 (Hamid: wariant z oud), Rozdz. 7 T1 przy forcie | gentle rustic medieval folk underscore, hurdy-gurdy soft drone, wooden flute, light frame drum, warm and tired morning mood, 80 bpm, earthy, understated, not cheerful, instrumental (Ch.4 variant: + with a faint oud and desert hand drum colour) |
| 5 | Anselm's Pages | Rozdz. 6 strony Anselma (kaplica T2 = BEZ muzyki, sama woda) | solemn monastic ambient underscore, low male choir hum without words, glass harmonium pad, slow 50 bpm, archaic and distant, reverberant stone chapel, instrumental, no lyrics |
| 6 | No Drums Tonight | Rozdz. 8 świt po najeździe, Rozdz. 11a krąg przed bełtem (8 s „nudy”) | quiet investigative medieval underscore, muted cello pizzicato, low sustained drone, occasional wooden creak percussion, suspicious and restrained, 65 bpm, dark but not action, instrumental |
| 7 | Field of the Answered | Rozdz. 10 sala tronowa Garroka (najdłuższa), Rozdz. 12 szamanka | grief ambient underscore, deep throat-singing drone, bowed bass, slow frame drum like a funeral step, somber and dignified, 48 bpm, tribal but mournful, no aggression, cinematic, instrumental, no words |
| 8 | Garrok at the Gate | Rozdz. 5 Parlament z Pashą, Rozdz. 7 Statki (Crane), Rozdz. 12 Ultimatum | tense diplomatic standoff underscore, low taiko pulse held back, bowed nyckelharpa drone, metallic scrape texture, restrained, 60 bpm, dark medieval, no climax, instrumental |
| 9a | Ring | Epilogue RING | bittersweet medieval ending underscore, warm strings and lute, ceremonial but hollow, a distant bell toll every few bars, faint war drums under the final section, 65 bpm, cinematic, instrumental |
| 9b | Break | Epilogue BREAK | quiet medieval resolution underscore, solo wooden flute over soft sea ambience, no percussion, no bell, peaceful and empty, 55 bpm, minimal, long decay, instrumental |

| 10 | Under the Helm | Rozdz. 8 Demaskowanie (od zdjęcia hełmu — wcześniej cisza), Rozdz. 11b Koperta i wyznanie | intimate medieval underscore, solo cello and distant soft piano, very slow 50 bpm, exposed and still, no percussion, no drone, a single melody that never resolves, cinematic, instrumental, no vocals |
| 11 | Bar It | Rozdz. 11c Śmierć Wata (pod slow-mo, kamera Maud) | medieval elegy underscore, low strings swell then fall away to a single held viola note, muffled war drums fading out, 45 bpm, grief without triumph, cinematic, instrumental, no vocals |
| 12 | The Reeve | Akt IV gameplay Maud (zbieranie sojuszników) | sparse medieval underscore, bowed nyckelharpa and quiet frame drum, resolute and tired, no ornament, 62 bpm, grey morning, instrumental, no vocals |
Tips: append `intro is just the drone` to keep the first 30 s melody-free; 2–3 min renders, cut with crossfade (cutscenes ≤ 45 s).

## Akt I — rozpiska muzyki per scena (2026-10-14, nadpisuje tabelę wyżej dla Aktu I)

Zasada filmowa: muzyka wchodzi **po** pointach i przechodzi pod dialog, a nie stoi na żartach. Żart ląduje w ciszy.
**Muzyka nigdy nie akcentuje dzwonu** (§1.5 scenariusza): żadnego bell-drona, żadnego „hitu” na ujęciu wieży, a bębny
po dzwonie są diegetyczne (z lasu), nie w partyturze. Dlatego #3 „Dusk Bell” nie idzie do Rozdz. 1. Zamiast niego jest A1-4.

| Scena | Muzyka | Poziom / wejście |
|---|---|---|
| P.0 Kajuta (Harrow) | **A1-1 Below Deck** | nie leci pod żadną kwestią. Jedna nuta w ciszy po sprzączce, zanim padnie list; potem nic — hymn przez pokład jest jedyną muzyką. Dwie nuty dopiero na ostatnim ujęciu, kiedy Harrow zostaje sam, ścięte czernią, i tylko gdy hymn już zszedł. Część grywalna bez partytury |
| P.1 Piorun | **cisza** | zero partytury: deszcz, kadłub, dzwon okrętowy jako jeden z pięciu dźwięków; każdy błysk ma twarde cięcie dźwięku |
| Plaża (walka) | muzyka bojowa z gry | — |
| P.2 Rozkazy Maud | **A1-2 Wreck Cove** | wchodzi po „Nobody argues with a man who lived through that.” pod czytanie listu, ~-26 dB; **urywa się** przed „A bell? We haven't got a roof.” (żart w ciszy), wraca na drugą kopertę |
| Marsz King's Road (T1) | **cisza** | wiatr, kroki, ptaki; żarty Maud/Pip na sucho |
| P.3 Fort | **cisza** do „…a wall and a half.” | po linii Maud wchodzi **A1-3 The Toll (karta aktu)** pod tytuł „ACT I — THE TOLL” |
| Rozdz. 1 Pierwszy dzwon | **A1-4 Walls at Dusk** | partytura idzie za **wsią** (ludzie zajmują mury), ~-24 dB; dzwon jest tylko diegetyczny; muzyka wycisza się pod koniec, zanim wejdą bębny z lasu (bębny bez muzyki) |
| Rozdz. 1 ognisko o świcie | **cisza** | ogień; pierwsza linia Wata w grze w ciszy |
| Rozdz. 2 farma (cutscenka) | `farm_dawn_music.ogg` | jest |
| Rozdz. 2 ognisko z Farmerem | **cisza** | |
| Rozdz. 3 izba z gorączką Pipa | **cisza** | deszcz o okiennice |
| Rozdz. 3 cicha noc | **cisza** | zwykły nocny ambient, świerszcze; **nie** robić z niej „niesamowitej” ciszy (to ziarno) |
| Rozdz. 3 poranek „Not a one!” | **A1-5 Three Stalls and a Goat** | wchodzi po „You broke their ritual, Ser Godric.”, ~-24 dB, gra ciągle także przez spojrzenie Maud na wieżę, gaśnie na „Dig.” |
| Rozdz. 4 klatka / Hamid | **A1-5b (wariant z oudem)** | po uwolnieniu, ~-26 dB pod „Three weeks…” |
| Rozdz. 4 Hamid o zmierzchu | **bez partytury** | jego nucenie (`a1_hamid_09`) jest muzyką |
| Karta „ACT II — THE WILDS” | **A1-3** (ten sam sting) | |

Lyrics we wszystkich: `[instrumental]`. Exclude styles: `vocals, choir, epic, trailer, bells, drums` (A1-5: bez `drums`).

| # | Nazwa | Suno style |
|---|---|---|
| A1-1 | Below Deck | solo low cello, dry close-mic, a few separate notes then real silence, no drone, no pad, no ambience, no rain, no percussion, no melody, no hall, medieval, instrumental, the rests are the piece |
| A1-2 | Wreck Cove | sparse medieval ambient underscore, solo bowed nyckelharpa over low cello drone, soft sea wash, no percussion, 60 bpm, grey dawn, numb rather than sad, lots of silence between phrases, cinematic, instrumental |
| A1-3 | The Toll (act card) | short medieval title sting, one low string chord swelling and fading, a single held viola note above, no melody, no percussion, dark and quiet, 15 seconds, cinematic, instrumental |
| A1-4 | Walls at Dusk | quiet communal medieval underscore, low strings and hurdy-gurdy drone, slow rising line, people getting ready at dusk, steady not heroic, no percussion, 62 bpm, fades to nothing at the end, instrumental |
| A1-5 | Three Stalls and a Goat | gentle rustic medieval folk, soft hurdy-gurdy, wooden flute, light frame drum, relieved tired morning, quietly hopeful, 80 bpm, understated, not cheerful, instrumental |
| A1-5b | Three Stalls (oud) | gentle rustic medieval folk, faint oud and soft desert hand drum over hurdy-gurdy drone, tired and warm, 78 bpm, understated, instrumental |

### A1-1 — wklejka

Poprzedni prompt (wiolonczela + skrzypienie + deszcz, „weary”) schodził w podkład. Deszcz i kadłub
już są w miksie kajuty. Ten utwór ma być tylko nutami i przerwami. Exclude:
`vocals, choir, pad, drone, ambient, rain, ocean, epic, trailer, orchestra, reverb`.

Style:

```text
solo low cello, dry close-mic, a few separate notes then real silence, no drone, no pad, no ambience, no rain, no percussion, no melody, no hall, medieval, instrumental, the rests are the piece
```

Lyrics:

```text
[Instrumental]

[Silence]

[One low note. Stop completely.]

[Silence]

[Two quiet notes. End. Silence.]

[Silence]

[One short note, no swell. Silence until the end.]
```

Suno i tak często dosypuje pad. Take z ciągłym tonem wyrzucić. Zostawić same nuty i wyciąć
cokolwiek stoi między nimi. W filmie nuta nie wchodzi pod tekst ani razem z hymnem: jedna po
kliknięciu sprzączki, zanim Harrow odezwie się o liście; dwie na samym końcu, na Harrowie
samym, ucięte czernią, bez crescenda.

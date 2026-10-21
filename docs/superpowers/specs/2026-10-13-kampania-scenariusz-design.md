# Warbell — scenariusz kampanii: sceny, dialogi, reżyseria (draft v2)

**Status:** 2026-10-14 v2.1 — pod master spec `2026-10-10-kampania-fabula-design.md` (v2) z dwiema
decyzjami autora z 2026-10-14 (naniesione też w masterze): **(1) prolog zaczyna się na statku i gracz od
początku wie, że jest giermkiem** — zwrot A jest tajemnicą przed *światem*, nie przed graczem (ironia
dramatyczna zamiast zagadki); **(2) dzwon ma umknąć** — żadna scena przed Rozdz. 10 nie jest *o* dzwonie,
każde ziarno dostaje głośniejsze fałszywe wyjaśnienie (§1.5 i §3b). Ten dokument **nie zmienia** fabuły, obsady ani
struktury z mastera — rozpisuje je na sceny, ujęcia i linie. Gdzie master coś ustala, master wygrywa.
Trailer i stara 3-nocna kampania (`crates/core/src/campaign.rs`) są obsolete i nie są tu źródłem.

Tekst w grze: angielski. Dokument: polski.

---

## 0. Ograniczenia, pod które to jest napisane

**Poziomy scen** (master §8): **T0** karta/napis + VO, gracz ma kontrolę · **T1** walk-and-talk /
ognisko, VO 2–3 postaci bez zmiany kamery (80% fabuły) · **T2** letterbox, 3–6 ujęć, aktorzy, VO,
skip spacją (~16 scen, §12.5 mastera, ~15–18 min łącznie).

**Twarze.** Brak lip-synca i mimiki. Gracz widzi twarz Wata w prologu (na statku, jako giermek) i
w scenach prywatnych; **świat** widzi ją dopiero w Rozdz. 8 (Idris) i publicznie w Rozdz. 11. Hełm w
kadrze = „kto patrzy” — przy świadkach kamera trzyma hełm, plecy, dłonie; sam na sam może pokazać twarz.

**Animacje postaci** — trzy pule, każda akcja aktora w tym dokumencie ma tag:

| Tag | Źródło | Status | Co daje |
|---|---|---|---|
| `[A]` | Quaternius UAL 1+2 Standard, CC0, już w `assets/npc/*.glb` (48 klipów × 14 NPC) | gotowe | Idle/Walk/Jog/Sprint/Walk_Formal/Walk_Carry/Push/Crouch, Idle_Talking, Sitting_Enter/Idle/Talking/Exit, LayToIdle, Idle_FoldArms, Yes, Idle_No, Idle_Rail_Call/Loop, Idle_Torch/Lantern, Interact, PickUp_Table, Chest_Open, Consume, Fixing_Kneeling, Farm_*, TreeChopping, Sword_*, Punch_*, Hit_Chest/Head/Knockback, Death01, Roll, Dance, Spell_* |
| `[B]` | Mixamo (darmowe, royalty-free w grach, **nie** redystrybuować FBX) → `tools/npc_mpfb/bake_anims.py` (retarget po kierunku kości) → `NPC_X_*` → re-export 14 NPC | ~1 dzień | Kneel, Bow, Salute, Point, Wave, Beckon, Clap, Cheer, Shrug, Look_Around, Hands_Up, Death02/03, Collapse, Stumble, Sleeping, Drinking_Sit |
| `[C]` | nasze: prop/fizyka/kamera/keyframe | per scena | dzwon (`castle::swing_bell` jest), walący się spichlerz, ogień, wrak, fala zabierająca ciało, hełm zdejmowany (swap mesh + 3 klatki dłoni), serce dzwonu (prop), koperta (prop), młoty na dzwon |

Reguły pisania pod pule: **żadnych dwuosobowych animacji kontaktowych** (uścisk, wręczenie — tylko
cięciem: prop znika u A, jest u B). Klipy są in-place; ruch daje transform. Rekwizyty w dłoni: tylko
pochodnia, latarnia, miecz, tarcza + nowe propy `[C]` przypięte do kości dłoni (koperta, serce dzwonu,
młot, kubek).

**Modele.** Maud = `elder_f` (w Akcie IV osobny grywalny model — koszt z mastera §6.4), Pip = `boy`,
Hamid = `rival_kit` kupiec w kaftanie, Crane = `merchant`/`priest` w czerni z łańcuchem, Idris =
`elder_m` w barwach pustyni, Garrok = Warlord (siedzący — potrzebny `[A:Sitting_Idle]` na orczym
rigu lub statyczna poza), Skarn = Hrolk-class berserker, szamanka = ork shaman wariant, Wat = hero w
hełmie / bez (swap), Harrow = ten sam hero model w kaftanie **tylko w falach, z daleka**.

---

## 1. Ton: jak to ma brzmieć

Master: *suchy, zmęczony, autoironiczny; żarty per akt od kpin przez zmęczenie do ciszy.* Rozwinięcie:

1. **Żart ma być mądrzejszy niż bohater.** Nie „sarkazm bo życie ciężkie”, tylko linia, która mówi
   coś prawdziwego o sytuacji szybciej, niż gracz zdążyłby to pomyśleć. Test: jeśli żart da się
   usunąć bez straty informacji — wylatuje.
2. **Maud kpi publicznie, żeby nikt nie patrzył uważnie.** Jej żarty są *funkcjonalne*: każdy
   odwraca uwagę od hełmu. Gracz dowie się o tym w Rozdz. 8 i wszystkie wcześniejsze żarty zmienią
   znaczenie. Pisać je tak, żeby to działało na drugim przejściu.
3. **Wat mówi o Ser Godricu w trzeciej osobie.** To jego cały humor: człowiek komentujący rolę,
   którą gra. „Ser Godric doesn't do stairs.” Nigdy nie przekracza granicy w żart o samej masce.
4. **Crane jest uprzejmy i to jest straszne.** Nigdy ironii, nigdy groźby wprost. Komedia bierze się
   z tego, że mówi dokładnie to, co robi, a brzmi jak pomoc.
5. **Dzwon ma umknąć.** To najważniejsza reguła dokumentu. Zwrot B („to dzwon ich wołał”) działa
   tylko, jeśli gracz w Rozdz. 10 ma *wow*, a nie *no przecież*. Dlatego:
   - **Żadna scena przed Rozdz. 10 nie jest o dzwonie.** Dzwon jest w tle: jedna z trzech rzeczy w
     ruinach, jeden z trzech edyktów, jeden rytuał dnia obok wypłaty i ogniska.
   - **Każde ziarno dostaje głośniejsze fałszywe wyjaśnienie** wypowiedziane przez NPC-a, który w to
     wierzy (lista w §3b). Gracz ma mieć do Rozdz. 10 trzy *własne* teorie: rytuał szamanów, Pasha,
     Korona — i wszystkie trzy są częściowo prawdziwe, więc nie wyglądają jak zmyłki.
   - Nikt **nie reaguje** na dzwon w kadrze. Hamid nie krzywi się „na dzwon” — ma ból głowy po
     klatce. Crane nie mówi o dzwonie — mówi o dyscyplinie. Pip mówi o dzwonie jak o *swoim* —
     to charakter, nie wskazówka.
   - Ziarna wizualne (inskrypcja na szkicu = inskrypcja na dzwonie, bębny po dzwonie) są **nigdy
     nie komentowane**. Kto zobaczy, zobaczy. Reszta dostanie to w Rozdz. 10 i wróci sprawdzić.
6. **Pip nie żartuje.** Jest jedyną szczerą osobą i dlatego jego linie bolą najbardziej.
7. **Orki nie są śmieszne.** Od Rozdz. 10 mówią prosto i rzeczowo; to ludzie są śmieszni, bo tego
   nie słyszeli.
8. **Jedna pointa na scenę.** Linia ≤ 14 słów. Scena T2 w limicie z §12.5 mastera.
9. **Akt IV nie ma żartów.** Maud bez ironii (master §9). Ostatni żart w grze pada w Rozdz. 11 i
   mówi go Wat przed śmiercią.

Kalibracja:
- MAUD: „Axe to the throat and the face. Nobody argues with a man who lived through that. Nobody
  looks at him too long, either.”
- WAT: „Ser Godric would like to be left alone.” MAUD: „Ser Godric would like a lot of things.”
- CRANE: „The Crown provides.” MAUD: „The Crown provides *forms*.” CRANE: „Forms are how the Crown
  knows you exist, Reeve.”
- HARROW: „Nobody looks at the man behind the shield, Wat. That's the point of you.”
- GARROK: „Four hundred and eleven times you rang. Four hundred and eleven times we came. Tell me
  again which of us is the fool.”

---

## 2. Obsada — karty głosu (skrót; pełne motywacje w masterze §3)

| Postać | Głos | Tik językowy |
|---|---|---|
| **Ser Godric Harrow** (prolog) | 50, głęboki, znudzony | mówi do Wata jak do mebla; jedno zdanie o Tessarine bez nazwy |
| **Wat** („Ser Godric”) | 40, chrypa (publicznie) / własny głos (prywatnie) | mówi o Ser Godricu w 3. os.; nie czyta; krótkie zdania; nigdy się nie tłumaczy |
| **Maud** (reeve) | 40–50, deadpan, ludowy | kpi z Wata publicznie; „Write it down.”; w Akcie IV zero ironii |
| **Pip** (12) | chłopięcy, szczery | mówi o dzwonie jak o osobie („she”) |
| **Hamid** | 30–40, ciepły, szybki, południe | targuje się o wszystko, także o prawdę; krzywi się na dzwon |
| **Crane** | miękki, wykształcony | nigdy nie podnosi głosu; „provides”, „a pity”, „see that…” |
| **Idris** | 60, baryton, gorycz | mówi o Tessarine bez nazwy („the city”) |
| **Garrok** | bardzo niski, wolny | liczby; prawo jako fakt, nie wiara |
| **Skarn** | młody, szybki, warczy | „iron” jako czasownik |
| **Szamanka** | stara, ork-mowa | mówi do Maud „widow” |
| **Anselm** | szept lektora | dziennik, coraz krótsze wpisy |

---

## 3. Sceny

Format: **Trigger** (z §12.3 mastera: PUSH / PULL / EDGE) → **T2 reżyseria** (ujęcia, klipy) →
**Dialog** → **T1/T0 wokół sceny**.

---

### PROLOG — „Landfall”

Prolog ma trzy ruchy: **statek** (gracz jest giermkiem i to widzi), **piorun** (cztery klatki, które
mówią wszystko bez jednego słowa), **plaża** (gracz w cudzej zbroi). Od P.2 świat zna „Ser Godrica”,
a gracz zna Wata. Cała reszta gry to ironia dramatyczna.

#### P.0 — T2 → grywalne „Na pokładzie” (90 s + ~2 min interakcji) · trigger: Nowa gra

Cel: pokazać Harrowa raz, z bliska, i dać graczowi *zrobić* coś jako giermek, zanim mu to zabierzemy.

1. Czerń. Skrzypienie, morze, deszcz na płótnie. Tytuł: *KELL — three days out*.
2. **Ujęcie 1** — kajuta, lampa `[A:Idle_Lantern_Loop]` kołysze się. Harrow siedzi `[A:Sitting_Idle_Loop]`
   w pełnej zbroi bez hełmu; hełm na stole. Twarz widać — jedyna osoba w prologu, której twarz jest
   w kadrze od razu. **Wat (gracz, twarz widoczna, kaftan) klęczy przy nim** `[A:Fixing_Kneeling]`
   i dopina nagolennik. Kamera za Watem, nisko — perspektywa giermka.
   > HARROW: Tighter.
   > WAT: There.
   > HARROW: Letter's on the table. Take it down to the Reeve.
   > *(hymn spod pokładu; Wat staje w drzwiach)*
   > HARROW: Forty peasants and a Reeve. Nobody looks at the man behind the shield, Wat.
   > They didn't look in the city, before it burned. That's the point of you.
   > WAT: Yes, Ser.
   > HARROW: Stop hovering.
   >
   > Tagi ElevenLabs i cięcia (werdykt o pasku, uraza do Korony, zagadka o mieście):
   > `docs/audio/elevenlabs-p0-poklad.md`.
3. **Interakcja (gracz steruje Watem, kamera TPS, kajuta + korytarz pod pokładem):** trzy drobne
   zadania giermka, każde jedno `E`: **zapiąć naramiennik** (Harrow: „Left. It's *always* the left.”
   — ziarno: Wat zna każdy pasek), **odnieść hełm na skrzynię** `[A:PickUp_Table]`, **zanieść list
   Reeve** — Maud pod pokładem `[A:Sitting_Talking_Loop]` z osadnikami; pierwsze spotkanie:
   > MAUD: You're his boy.
   > WAT: His squire.
   > MAUD: *(bierze list — cięcie, prop zmienia właściciela)* Same thing, from down here. Does he
   > ever come out?
   > WAT: He's resting.
   > MAUD: Resting. *(do osadników, głośniej)* The knight's resting. Sleep well, everyone, we're
   > guarded by a man who's *resting*.
   Po trzecim zadaniu Harrow: „Sleep, Wat. I'll want you at first light, and I'll want you *awake*.”
   Gracz idzie do hamaka → `[A:LayToIdle` wstecz`]` → czerń.

**Pointa P.0:** „That's the point of you.” — Harrow nieświadomie pisze Watowi instrukcję na całą grę.

#### P.1 — T2 „Piorun” (45 s) · trigger: automatycznie po P.0

Cztery klatki. Każda to **jedno ujęcie oświetlone jednym piorunem** (`[C]`: flash 0.3 s pełnej
jasności → 2 s ciemności z deszczem i dźwiękiem → kolejny). Między klatkami czerń, nie przejścia.
Zero dialogu. Zero muzyki — deszcz, drewno, i **gdzieś w tym** dzwon okrętowy, nierówny, jak każdy
dzwon alarmowy na każdym statku (nie eksponować; to jeden z pięciu dźwięków).

- **Czerń** — Wat się budzi (dźwięk: krzyk, dzwon okrętowy, trzask kadłuba). Napis: nic.
- **Klatka 1 — piorun:** pokład przechylony, rafa, marynarze `[A:Idle_Rail_Call]` przy dzwonie, woda
  przez burtę. Czerń.
- **Klatka 2 — piorun:** plaża, z perspektywy Wata na kolanach w piasku `[A:Crouch_Idle]`: **orki
  biegną na kamerę** `[A:Sprint_Loop ×6]` z pochodniami `[A:Idle_Torch_Loop]`, z lasu. Czerń.
- **Klatka 3 — piorun:** Harrow w zbroi, bez hełmu, **leży** `[A:Death01` ostatnia klatka`]`, topór
  obok, ork nad nim `[A:Sword_Idle]`. Maud w kadrze z boku, patrzy na Wata. Czerń.
- **Klatka 4 — piorun:** **dłonie Wata zapinają naramiennik** — zbliżenie, lewy pasek, ten sam co w
  P.0. Druga para dłoni (Maud) trzyma płytę. Hełm Harrowa na piasku, kamera na nim. Czerń.

Fade z czerni na gameplay: Wat w hełmie, miecz Harrowa, deszcz. Pierwszy ork. Tutorial walki.

**Gameplay plaży:** obóz orków z `camps.rs` (odpowiedź lokalna na dzwon okrętowy — master §6.2; gracz
tego nie wie, dla niego to „orki z lasu, bo wyspa”). Kamera TPS, hełm zamknięty. Wat **milczy**.
Jedyna linia to Maud, w trakcie walki, z tyłu:
> MAUD: *(do osadników)* Ser Godric's up! Behind him, all of you!

Gracz wie, że to nieprawda. Czterdziestu ludzi nie. To jest cała gra w jednym zdaniu.

**T0 po walce:** *„Hold the beach till dawn.”*

#### P.2 — T2 „Maud czyta rozkazy” (60 s) · trigger: koniec walki na plaży (PUSH)

Świt, popiół ognisk, 40 ludzi na piasku `[A:Sitting_Idle_Loop ×N]`, `[A:Idle_FoldArms]`. Maud stoi z
listem `[C: prop pergamin]` + `[A:Idle_Talking_Loop]`. Postać w hełmie siedzi na skrzyni, kamera **za
jej plecami**, twarzą do tłumu.

> MAUD: *(głośno, do wszystkich)* The sea took the squire. Wat. Some of you knew him. *(pauza
> dokładnie jednej długości oddechu)* Ser Godric took an axe on the beach. Throat and face. He'll
> speak when he's ready and not before. Nobody argues with a man who lived through that.
> MAUD: *(łamie pieczęć, czyta)* „Make for the old fort of Saint Ottilie. Restore it. Ring the
> King's bell at every dusk.” *(opuszcza list)* That's the whole order. The rest is who to thank.
> SETTLER: A bell? We haven't got a *roof*.
> MAUD: Then we'll ring it from the ground. *(podnosi drugą kopertę `[C: prop]`, czerwona pieczęć)*
> This one's „for the Lord Commissioner's eyes only.” *(do postaci)* Ser Godric keeps it. Ser
> Godric's eyes, since the blow — I'll read him what he needs.

Cięcie: koperta ląduje w satchelu (toast „Sealed dispatch — cannot be sold or dropped”).

Pointa sceny: „The rest is who to thank.” Gracz *wie*, że Maud właśnie rozdała role czterdziestu
świadkom w trzech zdaniach, i patrzy, czy ktoś się zorientuje. Nikt. Osadnik pyta o dach.

**T1 marsz King's Road** (walk-and-talk, kamera gracza, VO):
> PIP: *(dogania)* Is it true? About the axe?
> MAUD: Ask him.
> PIP: He doesn't talk.
> MAUD: Then that's your answer, isn't it.
> *(dalej, po 20 s)*
> PIP: Where's the bell?
> MAUD: In the fort, if the first lot didn't melt it for spoons.
> PIP: Who's the first lot?
> MAUD: People who had a roof.

#### P.3 — T2 „Fort i dzwon w wieży” (30 s) · trigger: strefa fortu (PULL)

Ruiny. Kamera niska, przez bramę bez wrót, tilt w górę na kamienną wieżę: w niej dzwon, cały,
patyna, sznur zgniły. Pip pierwszy przy sznurze `[A:Idle_Rail_Loop]`, głowa zadarta.

> PIP: *(biegnie od rzeczy do rzeczy)* There's a well! There's a — that's a chapel, that was a
> chapel — there's a *bell*. Up there. It's whole.
> MAUD: The well's whole too. Start with the well.
> PIP: Can I—
> MAUD: At dusk. King's orders, one of them. *(rozgląda się po ruinie)* God help us. A well, a
> bell, and a wall and a half.

Tytuł na czerni: **ACT I — THE TOLL**.

---

### AKT I — „The Toll”

#### Rozdz. 1 „Stones” — T2 „Pierwszy dzwon” (20 s) · trigger: pierwszy zmierzch (EDGE)

Jedyna scena T2 Aktu I. Pip na platformie `[A:Idle_Rail_Call]`, wieś w dole patrzy w górę
`[A:Idle_Loop ×N]`, postać w hełmie na murze, plecami do kamery, miecz w dłoni `[A:Sword_Idle]`.
Dzwon `[C: swing_bell]`. Kamera odjeżdża nad las — **ale scena jest o wsi, nie o dzwonie**: w dole
ludzie zajmują mury, ktoś gasi ognisko, Maud liczy głowy. Pochodnie w lesie zapalają się w tle,
między wyciem wilków a wiatrem; bębny wchodzą pod koniec, pod kamerą już na murze. Nikt nie patrzy w
las. Gracz, który patrzy, zobaczy. Reszta zobaczy w Rozdz. 10 i wróci.

> PIP: *(zdyszany, dumny)* Did you hear her? That's *mine*. She's the only thing on this island that
> does what it's told.
> MAUD: *(do Wata, cicho)* Go on. They're watching Ser Godric, not me.

**T1 ognisko (świt po nocy 1):**
> MAUD: Three walls, no gate, a bell. We're a church with ambitions.
> WAT: *(pierwsza linia w grze; chrypa)* Ser Godric doesn't do churches.
> MAUD: Ser Godric doesn't do *stairs*, I've noticed. Mind the shoulder strap. It's riding up.
> *(ziarno A: animacja poprawiania naramiennika `[C: 3-klatkowy klip dłoni]`)*

#### Rozdz. 2 „Torches” — tylko T1 / T0

T0: *„Torch-bearers tonight. Keep them off the fields.”*
T1 ognisko:
> FARMER: You said hold the line. Line's fine. *Barn's* gone.
> WAT: Ser Godric said hold the *line*.
> FARMER: Ser Godric should've said hold the barn.
> MAUD: Write it down. Next time he'll say barn.

#### Rozdz. 3 „The Ritual” — T1 (PUSH: gorączka Pipa)

Świt. Pip leży w izbie `[A:LayToIdle` klatka 0`]`, Maud siedzi `[A:Sitting_Idle_Loop]`. Wat w drzwiach.
> MAUD: Swamp fever. It's one night. Someone else can pull a rope.
> WAT: Ser Godric can't climb the tower.
> MAUD: Ser Godric can't climb the tower in front of *forty people*. Different thing.
> PIP: *(z posłania, słabo)* Don't let anyone else ring her.
> MAUD: *(po pauzie)* …Fine. One night. The King can write me a letter.

**Ten sam dzień** gracz rozbija obóz szamanów (tytułowy „Ritual”). **Noc cicha** (oskryptowana,
`siege.rs` noc bez fali). Rano — i to jest kluczowe — **wieś ma gotowe wyjaśnienie, które brzmi
lepiej niż prawda:**
> SETTLER: Not a one! Not a single one. You broke their *ritual*, Ser Godric. That's what it was.
> That's what brought them. Burn the shamans, the rest go home.
> MAUD: *(patrzy na wieżę; jedna sekunda; potem na osadnika)* …Could be. Dig.

Od tej chwili „rytuał szamanów” to **oficjalna teoria wsi** i barki ją powtarzają do Rozdz. 10
(„Another camp burned, another quiet night coming, you'll see”). Gracz, który pali obozy, dostaje
*pozorne* potwierdzenie: po każdym spalonym obozie noc jest lżejsza (bo lokalne obozy to część
fali — prawdziwy efekt, fałszywa przyczyna). Dzwon nie pada w tej scenie ani razu.

#### Rozdz. 4 „The Muster” — T1 po uwolnieniu (Hamid)

Klatki (`camps::spawn_cage`). Hamid w środku `[A:Sitting_Idle_Loop]`, wstaje `[A:Sitting_Exit]`,
chwieje się `[B:Stumble]` (do czasu: `[A:Walk_Loop` speed 0.5`]`).
> HAMID: Three weeks. They fed me. Badly, but they fed me. You don't feed what you mean to eat.
> WAT: You're the Pasha's man.
> HAMID: I'm the Pasha's *caravan*. The man is in the desert, and he's going to ask me what I
> was worth. *(patrzy na hełm)* …Ser Godric Harrow. He'll want to hear that too.

Hamid zostaje w wiosce (ranny). **Ziarno B, zakopane:** Hamid ma „ból głowy po klatce” i o zmierzchu
siedzi w izbie, nie na dziedzińcu `[A:Sitting_Idle_Loop]`. Nuci. T1 jeśli gracz podejdzie:
> HAMID: Head. Three weeks of their drums, knight, you'd have a head too.
> WAT: What's the song?
> HAMID: Theirs. They sing in the evenings. *(pauza)* Not what you'd expect. Sad. Like somebody's
> leaving for work.
Gracz słyszy: anegdota z niewoli. Hamid „ma ból głowy o zmierzchu” przez całą grę; nikt tego nie
łączy. Drugie ziarno Hamida (zna ich mowę) pada mimochodem w Rozdz. 5 w drodze, w żarcie o cenach.

Tytuł: **ACT II — THE WILDS**.

---

### AKT II — „The Wilds”

#### Rozdz. 5 „Sand and Salt” — T2 „Parlament z Pashą” (90–120 s) · trigger: strefa białej flagi (PULL)

**T1 w drodze (Hamid):**
> HAMID: Rule one with the Pasha: he talks first. Rule two: you don't. Rule three: you have a
> helmet, so you've already broken rule two in his eyes.
> WAT: Ser Godric keeps the helmet.
> HAMID: *(westchnienie)* Then I'll talk for the both of you. Cheaper.

**T2.** Namiot pod białą flagą na granicy pustyni. Idris siedzi `[A:Sitting_Idle_Loop]` — nie wstaje
na powitanie, to jest cały jego komentarz. Dwóch strażników `[A:Idle_Shield_Loop]`. Wat stoi,
kamera **nisko za Idrisem** — widzimy hełm z perspektywy człowieka, który będzie go oceniał. Hamid
między nimi `[A:Idle_Talking_Loop]`.

> IDRIS: Hamid. You're thinner.
> HAMID: Orks, lord. They don't salt anything.
> IDRIS: *(do Wata)* Ser Godric Harrow. Hero of the eastern campaign. You've been burning
> orkish camps since you landed. I'd like you to stop.
> WAT: *(chrypa)* They come to us.
> IDRIS: They come to *everyone* now. My caravans, my wells, my herds. Thirty years I've shared
> this island with them and never lost a mule. Then your ships, your bell, your knight — and now
> they fight on every road. You bring war the way other men bring luggage.
> WAT: *(nic)*
> IDRIS: Nothing? *(wstaje `[A:Sitting_Exit]`; kamera podnosi się z nim i po raz pierwszy jest
> wyżej niż hełm)* The Harrow I remember had more to say. *(bliżej)* I knew the man who wore that
> crest. I met him once. The day before the city burned.
> *(Wat cofa się o pół kroku — `[A:Walk_Loop` 1 klatka wstecz`]`. To jedyna rzecz, którą Idris
> potrzebował zobaczyć.)*
> IDRIS: *(do Hamida)* The salt stops. The caravans stop. Tell your knight, since he doesn't
> hear me: next time the war comes to *him*, it'll be mine, and it will be polite.

**Pointa:** „You bring war the way other men bring luggage.” Idris ma rację co do skutku i myli się
co do przyczyny; gracz, który pamięta cichą noc, już czuje, że obie strony mówią prawdę.

**T1 powrót:**
> HAMID: He liked you.
> WAT: He threatened us.
> HAMID: *Politely*. You don't know the Pasha. That was a courtship.

Od tej sceny `rival.rs` → `Warned` → `Harassing` (niekrwawe wypady, master §6.4).

#### Rozdz. 6 „The Old Colony” — T2 „Zatopiona kaplica” (45 s) · trigger: strefa (PULL) + T0 strony Anselma

**Strony Anselma** (podniesienie `[A:PickUp_Table]` → karta pergaminu + szept lektora). Pięć, po
biomach (`wyspa-opowiada`), kolejność dowolna. Pisane tak, by każda była o czymś innym, a razem
składały zdanie, którego żadna nie mówi.

Zasada: **strony są kroniką kolonii, nie śledztwem.** Czytelnik ma polubić Anselma i jego
Gubernatora, dowiedzieć się o soli, gorączce i Starych, a dzwon ma być wzmianką liturgiczną.

1. **Las — lądowanie.** *„Day six. Good timber, no natives — so says the Captain, who has walked as
   far as the beach. I have walked to the ridge. There are paths up here, and they are not ours.
   Two men down with the sweating fever already. The Governor says it will pass with the dry.”*
2. **Pustynia — piramida.** *„The southern people trade salt for iron and will not cross the stones
   after dark. They say the stones listen. The Captain says the stones are stones. I have begun to
   prefer the southern people. The fever has not passed with the dry.”*
3. **Skały — wieża.** *„The tower is finished and the bell hung for the Hours, so the Crown may hear
   them kept even here. Brother Oswin has the Vespers and takes it very seriously. Fourteen dead of
   the fever now. The marsh people have begun to come at night; the Captain says it is the fever
   drawing them, as wolves to a sick herd. I do not think wolves count.”*
4. **Śnieg — pustelnia.** *„I have gone north. I have stopped most things. They have stopped coming
   — or I have stopped being worth the walk. I set this down for someone cleverer than me. I have
   had a year to understand it and I only understand that it is simple, and that the Governor did
   not want it to be.”*
5. **Bagno — kaplica** (ostatnia, zalana — scena T2 poniżej).

Uwaga: **gorączka jest prawdziwa.** Crane w Rozdz. 7 mówi „fever” i strony to *potwierdzają* — gracz,
który czytał, nie łapie go na kłamstwie, tylko na *niepełnej prawdzie*. To lepsze ziarno niż kłamstwo.

**T2 Kaplica.** Woda po kolana, światło z góry, dzwon na szkicu. Kamera: zbliżenie na stronę w
dłoni Wata `[A:PickUp_Table]`; **inskrypcja na szkicu** jest tą samą, którą gracz widzi na dzwonie
w forcie (ten sam font/decal). Tekst strony pokazany z rozmytym środkiem:

> *„…came only after we rang ▒▒▒▒▒▒▒▒▒▒▒▒▒ the Governor will not let us stop ▒▒▒▒▒▒”*

Maud (jeśli w drużynie; w przeciwnym razie czyta Hamid):
> MAUD: *(czyta na głos, bo Wat „nie widzi”)* „…came only after we rang…” Water. „…the Governor
> will not let us stop…” Water. *(obraca stronę)* The rest's gone.
> WAT: Rang what?
> MAUD: Doesn't say. The Hours, I'd think. Monks. *(patrzy na szkic; krótko)* He drew the tower.
> Decent hand. *(składa stronę)* He didn't like his Governor much, did he.
> WAT: Nobody likes their Governor.

Scena kończy się na *Gubernatorze*, nie na dzwonie — i tak gracz ją zapamięta: „ktoś nie dał im
przestać”. Co „przestać” — dopowie Garrok. Inskrypcja na szkicu jest w kadrze 2 sekundy, nie jest
komentowana. Zero muzyki w tej scenie. Tylko woda.

#### Rozdz. 7 „The Commissioner” — T2 „Statki w zatoce” (60–90 s) · trigger: świt po Rozdz. 6, limit 2 nocy (PUSH)

Wreck Cove. Trzy statki `[C: prop]`, wrak nadal na rafie w tle (to ważne: Korona cumuje obok
własnej katastrofy i nie komentuje). Trap, żołnierze w szeregu `[A:Idle_Shield_Loop ×8]`, kusznicy.
Crane schodzi `[A:Walk_Formal_Loop]`, zatrzymuje się na piasku, **patrzy na wrak dokładnie jedną
sekundę** `[B:Look_Around]` (do czasu: obrót głowy przez transform), potem na Wata.

> CRANE: Ser Godric. The Crown provides. *(gest na skrzynie `[B:Point]` / do czasu `[A:Interact]`)*
> Grain, physic, thirty men, and the King's gratitude, which weighs the least and costs the most.
> MAUD: The Crown provides *forms*.
> CRANE: Forms are how the Crown knows you exist, Reeve. *(do Wata)* Your dispatches?
> WAT: Lost. In the wreck.
> CRANE: *(nie patrzy na wrak)* A pity. *(uśmiech w głosie)* Then we'll start fresh. Three edicts,
> effective this dusk. *(odlicza na palcach `[A:Interact]`)* A tithe of grain to the Crown stores.
> A curfew after the King's bell. And the Hours kept — bell at dusk, no exceptions, not fever nor
> fog nor a Reeve with a good heart.
> MAUD: *(bez emocji)* He's heard about the fever.
> CRANE: The Crown hears everything, eventually. It's the *eventually* that keeps me employed.

Trzeci edykt jest trzeci, nie pierwszy; gracz zapamięta dziesięcinę, bo ta boli w ekonomii.

**Pointa:** „A pity.” — on wie, co jest w kopercie. Gracz nie. Jeszcze.

**T1 przy forcie (ten sam dzień, Crane ogląda ruiny):**
> CRANE: The first colony. Dreadful business. Fever took them, the records say.
> WAT: The chronicler wrote about orks.
> CRANE: Chroniclers write what sells, Ser Godric. Fever doesn't sell. *(pauza)* Neither does
> truth, as a rule, but one lives in hope.

**Noc incydentu (oskryptowana, T1 + barki):** żołnierze Korony piją na plaży; jeden wyławia dzwon
okrętowy z piasku i bije w niego do pijackiej piosenki — **jeden z wielu hałasów tej nocy** (bębny,
śpiew, strzały na wiwat). Obóz Korony zostaje zaatakowany przez orki z najbliższego obozu (master
§6.2). Rano, T1 — i scena jest o *dyscyplinie*:
> CRANE: *(spokojnie, do sierżanta)* Flog the singers. No drink on the beach, no fires above
> knee-height, and nobody makes a noise after dark that I didn't order. *(odchodzi)* We are not
> *peasants*, Sergeant.
> SERGEANT: Sir.
> *(Wat słyszy to z boku. Brzmi jak oficer po złej nocy. Jest oficerem po złej nocy. Tylko gracz,
> który pamięta cichą noc Pipa, może zauważyć, czego Crane NIE zrobił: nie zapytał, skąd orki.)*

**Edykty (T0 karta o świcie, od teraz):** przyjmij/odrzuć, efekt na ekonomię/noce. Teksty edyktów
w jednym rejestrze: *„By order of the Lord Commissioner: …”* + jedno zdanie, które brzmi jak pomoc i
jest poborem.

#### Rozdz. 8 „Northern Dog” — noc fałszywej flagi + T2 „Demaskowanie” (120 s)

**Noc (oskryptowana):** najazd „pustynnych” żołnierzy; spichlerz płonie `[C: mesh collapse + fire]`,
sześciu zabitych `[A:Death01]` + `[B:Death02]`. Rano w popiele leży szabla pustynna `[C: prop]`.

**T1 świt, tłum:**
> SETTLER: Six. *Six.* The desert did this.
> HAMID: The desert doesn't kill like this. Idris takes sheep. He doesn't take *people*, he's
> too cheap for the burial.
> SETTLER: You'd say that.
> HAMID: I *would*. I'm also right. Those two things happen together more often than you'd think.
> CRANE: *(spokojnie)* Thirty men are at Ser Godric's disposal. The Crown does not tell a knight
> where to point them. The Crown merely notes where the smoke is.

**T2 Demaskowanie** · trigger: sala tronowa Pashy po zdobyciu twierdzy (PULL). W sali: Idris na
kolanach `[B:Kneel]` (do czasu `[A:Fixing_Kneeling]`), Maud, Crane, trzech żołnierzy Korony. **Brak
wieśniaków** (master §11/14). Kamera zaczyna za Idrisem, nisko.

> IDRIS: My men did not burn your granary. I'd have burned it *in daylight*, so you'd know whose
> it was.
> WAT: *(nic)*
> IDRIS: *(patrzy w górę na hełm; długo)* And you are not Harrow. Harrow was taller. Harrow would
> have killed me already. Harrow did not *fidget*. *(cicho)* Who are you?
> *(Cisza. Crane nie rusza się. Maud zamyka oczy.)*
> *(Wat podnosi ręce do hełmu. Kamera: **tylko dłonie** przez trzy sekundy `[C: swap mesh +
> 3-klatkowy klip]` — ten sam lewy pasek co w P.0 i w klatce 4 prologu. Potem cięcie na reakcję
> Idrisa, Crane'a, Maud — **świat widzi twarz po raz pierwszy**; gracz zna ją od statku, więc
> ujęcie jest o *nich*, nie o nim. Bez muzyki.)*
> WAT: *(bez chrypy — pierwszy raz normalnym głosem)* Wat. I carried his shield.
> IDRIS: *(prawie śmiech)* A footman. The Crown sent me a *footman*.
> WAT: The Crown didn't send me. The sea did. The Crown sent *him*. He's in it.
> CRANE: *(uprzejmie)* Ser Godric is tired. The Pasha's fate is Ser Godric's to decide, as his
> men expect. *(nacisk na „as his men expect” — to szantaż, wypowiedziany jak grzeczność)*

**Wybór** (HUD): *Spare the Pasha* / *Execute the Pasha*.
- **Spare:** Idris: „Then go home, footman. And find out who sold you the sword.” → `Spared`.
- **Execute:** Wat musi to zrobić sam, w kadrze, bez hełmu `[A:Sword_Attack]`; Maud odwraca się.
  Crane: „Decisive. The Crown will note it.” → `Executed`. (Hamid zostaje, ale od tej chwili nie
  żartuje.)

Hełm wraca na głowę w ostatnim ujęciu `[C]`. Tytuł: **ACT III — THE HOLD**.

---

### AKT III — „The Hold”

#### Rozdz. 9 „Under the Helm” — tylko T1

**Ognisko Wat–Maud, prywatnie, Wat bez hełmu** (model swap w strefie ogniska bez świadków; master
§6.4). Najdłuższa rozmowa T1 w grze (~90 s), skip-owalna. Gracz zna twarz Wata od P.0 — ta scena
nie *ujawnia*, tylko pozwala mu wreszcie *mówić* własnym głosem.
> MAUD: You fidget. He's right.
> WAT: He had fifteen years of me fidgeting behind him. I know what it looks like from the front
> now.
> MAUD: *(pauza)* Tessarine.
> WAT: He took the surrender. Then he took the city. I stood two paces back and held the shield
> straight. Very straight. I was good at it.
> MAUD: And now?
> WAT: Now I'm standing two paces back from *him*. *(wskazuje hełm przy sobie)* Just the other
> side.
> MAUD: Forty people think he's a hero.
> WAT: Forty people are alive. I'll take the trade.
> MAUD: *(wstaje)* Put it on. Someone's coming.
> WAT: *(zakłada; chrypa wraca)* …Ser Godric thanks the Reeve for the fire.
> MAUD: Ser Godric can chop the next lot.

**Crane w cztery oczy (PUSH):**
> CRANE: A deserter in a dead knight's armour. The Crown hangs men for less. *(bez pauzy, tym
> samym tonem)* But the Crown needs Ser Godric. Gnashfang falls this month. Rams at dawn,
> ballistae by the week's end. *(uśmiech)* You'll find I'm very easy to work with, Wat. I only
> ever ask for what I've already arranged.

Edykty zaostrzają się: pobór, dzwon też o świcie, jeńcy na statki (T0 karty).

#### Rozdz. 10 „Gnashfang” — T2 „Sala tronowa, Garrok” (2–3 min, najdłuższa) · trigger: strefa tronu po wyłomie (PULL)

**Przed sceną (gameplay, oskryptowane):** garnizon przy bramie jest *mały* (orki wykrwawione — plan
działa). Za bramą miasto: chaty, kotły, starcy `[A:Sitting_Idle_Loop]`, młode orki **uciekają do nor**
`[A:Sprint_Loop]`. Barki tłumaczone **jeszcze nie są**. Gracz idzie przez miasto bez walki, jeśli
nie zaatakuje.

**T2.** Sala. Garrok na tronie, jedno oko, nie wstaje, nie ma broni. Hamid wchodzi za Watem i
Maud. Dwóch strażników Garroka stoi — i **opuszcza broń**, gdy Garrok mówi. Kamera: szeroko, potem
coraz bliżej Garroka, nigdy bliżej Wata.

> GARROK: *(ork-mowa, warkot — 4 s bez napisów)*
> HAMID: *(tłumaczy; od tej linii **napisy pod ork-mową włączają się na stałe** — warkot Garroka
> dostaje tekst w trakcie, nie po)* He says: „You rang. We came. Sit, it's a long story and you've
> walked.”
> WAT: *(nie siada)*
> GARROK: *(od teraz łamana wspólna mowa, sam)* Four hundred and eleven times you rang. Four
> hundred and eleven times we came. Tell me again which of us is the fool.
> MAUD: You *count*?
> GARROK: Iron speaks, we answer. Same night. Law. A chief who does not answer — *(gest na syna,
> poza kadrem)* — is not chief by morning. *(do Wata)* You think I *want* your walls?
> WAT: You took our people.
> GARROK: We *kept* your people. Alive. For trade. Law says: the caller stops, the kept go home.
> *(pauza)* Nobody came to trade. You came with the sword. Every time. Four hundred and eleven.
> HAMID: *(cicho, do Wata)* I told you. They don't feed what they mean to eat.
> WAT: *(po długiej ciszy; chrypa)* Who rang first? A hundred years ago.
> GARROK: You did. *(wzrusza ramionami `[B:Shrug]` / `[A:Idle_No_Loop]`)* You always do. We only
> ever answer.
> MAUD: And if the bell stops?
> GARROK: Then the kept go home, and we do not come. *(patrzy na Wata)* Not for a bell. Only for
> blood on holy ground. That is the other law. Don't give us that one.
> WAT: Where?
> GARROK: The stones. Dawn. No iron. *(pierwsza linia, w której jest zmęczony, nie groźny)* I am
> old, knight. I would like to die of it.

**Pointa:** „You always do. We only ever answer.” Zwrot B wypowiedziany w całości — przez orka,
spokojnie, jak liczba. **Reżyseria „wow”:** po „Four hundred and eleven times you rang” — **cięcie
na 1,5 s montażu wstecz** `[C: 4 stopklatki z wcześniejszych scen]`: klatka 1 prologu (marynarze przy
dzwonie okrętowym), cicha noc (pusta wieża), szkic z kaplicy (inskrypcja), pijany żołnierz Crane'a z
dzwonem. Bez dźwięku. Powrót na Garroka. To jedyny montaż wsteczny w grze — gracz dostaje wszystkie
ziarna naraz i rozumie, że *widział je wszystkie*.

**T1 marsz powrotny (Hamid tłumaczy barki mijanych orków — napisy już działają):**
- ORK (mijany): „Not tonight. The iron voice again.”
- ORK (stara): „My brother answered last winter. He did not come back.”
- ORK (do młodych): „Tell the little ones we hunt. Not the other thing.”
> HAMID: You killed a few hundred of these. Must be strange, hearing them.
> WAT: *(nic)*
> HAMID: Good. Means it's working.

Od tej sceny: akcja *Silence the bell* (koszt: gniew Korony — edykt). Jeśli gracz uciszy: noc
**cicha**. Jeśli dzwoni: orki przychodzą, ale **barki mają napisy**. To ma boleć.

#### Rozdz. 11 „The Stones” — trzy sceny T2

**11a — T2 „Zasadzka w kręgu” (60 s)** · trigger: strefa kręgu o świcie (PULL). Jedyna scena T2
z walką w środku (master §12.3).

Krąg, mgła, świt. Garrok idzie bez broni `[A:Walk_Loop` speed 0.6`]`, dwóch towarzyszy. Wat, Maud,
Hamid. Crane **nie przyszedł** — gracz zauważy to dopiero po fakcie. Garrok staje. Kamera szeroko,
spokojnie, za długo — ma być nudno przez 8 sekund.

> GARROK: No iron. Good. *(patrzy na Wata)* Take the helmet off, knight. Holy ground. I want to
> see who I'm—

Bełt `[C]`. `[A:Hit_Chest]` → `[B:Collapse]` / `[A:Death01]`. Drugi, trzeci. Z zarośli — kusznicy
Korony `[A:Spell_Simple_Shoot` jako „strzał z kuszy” do czasu właściwego klipu`]`. Towarzysze Garroka
`[A:Sprint_Loop]`. **Walka** (oskryptowana: gracz vs kusznicy, albo ucieczka — oba kończą scenę).

Po walce, nad ciałem, Hamid klęczy `[A:Fixing_Kneeling]`:
> HAMID: *(tłumaczy ostatni warkot)* „…Skarn.” That's all. Just the name.
> MAUD: Crane.
> WAT: Crane.
> MAUD: He never came up here. He *never* comes up anywhere.

Z daleka, od strony Hold, gong `[C]` — ale inny: trzy uderzenia, nierówne. Hamid, blady:
> HAMID: That's not the call. That's the *other* law. Blood on holy ground. He doesn't need your
> bell any more.

**11b — T2 „Koperta i wyznanie” (90 s)** · trigger: powrót do fortu (PULL)

Dziedziniec. Cała wieś `[A:Idle_Loop ×N]`, żołnierze Korony pod murem, Crane z boku
`[A:Idle_FoldArms]`. Wat wyjmuje kopertę `[C: prop]` i podaje Maud (cięcie, nie animacja). Maud
łamie pieczęć `[A:Interact]`.

> MAUD: *(czyta; jej głos — po raz pierwszy — bez ironii)* „To the Lord Commissioner. The
> colonists are to ring at every dusk until the marsh is emptied of the ork. Losses among them are
> acceptable and expected. Thereafter the colony is to be turned upon the Pasha by any means. The
> island is granted, in consideration of this service, to Ser Godric Harrow and his heirs.”
> *(opuszcza list)* Signed. Harrow. In his own hand.
> *(Cisza. Pip w tłumie. Hamid. Crane — nie rusza się.)*
> WAT: *(zdejmuje hełm. Publicznie. `[C]`. Kamera: najpierw tłum, dopiero potem on.)*
> WAT: My name is Wat. I carried his shield. Ser Godric died on the beach — and he knew what
> this bell was for. He was going to get an island for it. *(pauza)* I was good at carrying the
> shield. I was less good at being him. You followed anyway, so I don't know what that says about
> either of us.
> *(ostatni żart w grze; nikt się nie śmieje)*
> WAT: There's no bell tonight. Not from me.
> CRANE: *(bez zmiany tonu)* The Crown does not reinforce deserters. *(do sierżanta)* The bay.
> All of them. *(do Wata, niemal ciepło)* It was a good run, Wat. Really.
> *(Żołnierze odchodzą `[A:Walk_Formal_Loop]`. Pip — cięcie — już go nie ma w tłumie. Cięcie na
> wieżę: Pip wyjmuje serce dzwonu `[C: prop odczepia się od kości dłoni]` i schodzi po drugiej
> stronie. Nikt tego nie widzi poza graczem.)*

**11c — T2 „Śmierć Wata” (45 s)** · trigger: oskryptowana (brama + czas / próg HP)

Noc. Skarn przychodzi mimo ciszy. Gameplay: utrzymaj bramę, aż licznik „ludzie w forcie” dojdzie
do 100%. Gdy dojdzie — scena.

Brama. Wat bez hełmu, sam, `[A:Sword_Block]` → `[A:Hit_Knockback]`. Skarn w kadrze z tyłu, nie
wchodzi w walkę jeden na jeden — to nie pojedynek bossowy, to tłum. Kamera z muru, z góry, **z
perspektywy Maud**.
> WAT: *(do bramy za sobą, nie odwracając się)* Bar it.
> MAUD (z muru): Wat—
> WAT: *Bar it*, Maud. Ser Godric would've said it nicer.
> *(zasuwa `[C]`. `[A:Death01]` — nie w zbliżeniu. Kamera zostaje na Maud. Slow-mo z
> `succession.rs`, ale **bez wisp** — tu nikt nie „dziedziczy”. Kamera po prostu nie wraca do Wata.)*

Świt. Orki się wycofują. Kamera jest kamerą Maud. HUD: imię „Maud”. Tytuł: **ACT IV — THE LAST
TOLL**.

---

### AKT IV — „The Last Toll” (gramy Maud; zero żartów)

#### Rozdz. 12 — T2 „Ultimatum Crane'a” (45 s) · trigger: świt po śmierci Wata (PUSH)

Brama, od zewnątrz. Crane sam, z białą chorągiewką, żołnierze w tle. Maud na murze.
> CRANE: Reeve. My condolences. He was braver than his papers.
> MAUD: *(nic)*
> CRANE: Ring the bell tonight. The Crown will hold your walls, as it always meant to. Don't, and
> the colony is in rebellion by the King's own statute, and I am *so* tired of statutes.
> MAUD: We haven't got a clapper.
> CRANE: *(pierwsza pauza w całej grze)* …Then find one, Reeve. By dusk.

**T1 — sojusznicy (PULL, dowolna kolejność):**

*Idris (jeśli żyje; Hamid zanosi list):*
> IDRIS: *(czyta; pauza)* „By any means.” *(składa)* The granary.
> HAMID: I said it.
> IDRIS: You did. *(do Maud)* Your footman spared me. I will not be worse than a footman. My men
> come at dusk. *(pauza)* He was braver than the knight. Tell them that, if there's anyone left
> to tell.

*Szamanka (wdowa po Garroku), pod kręgiem:*
> SHAMANESS: *(ork-mowa z napisami)* Widow. You are widow too, now. *(pauza)* My husband died
> without iron. Your man died without the bell. *(patrzy na wieżę)* If the iron voice speaks, we
> must come. Every clan. Mine too. Law. *(ciszej)* If it does not speak… then we see who we are
> without it. Nobody has seen that in a hundred years.

*Jeńcy uwolnieni (Hold):* bez dialogu — stoją przy bramie `[A:Idle_Loop]`, biorą broń
`[A:PickUp_Table]`. Jeden: „He kept us alive. Both of them did.”

#### T2 „Wieża: wybór” (60 s) · trigger: strefa wieży (PULL)

Pip siedzi na platformie `[A:Sitting_Idle_Loop]`, serce dzwonu na kolanach `[C: prop]`. Maud wchodzi.
> PIP: I hid her heart. Nobody can ring her without it. *(pauza)* Not even me.
> MAUD: I know.
> PIP: She's the only thing on this island that ever did what it was told. *(patrzy na dzwon)*
> That's the whole problem, isn't it.
> MAUD: *(siada obok)* Yes.
> PIP: *(podaje serce — cięcie, prop zmienia właściciela)* You decide. You're the Reeve. *(pauza)*
> I don't want to be the one who decides. I'm twelve.

**Wybór** (dwie akcje przy dzwonie, obie wymagają przytrzymania): **RING** / **BREAK**.

#### Epilog RING (2 min)

Pip wkłada serce `[C]` i dzwoni `[A:Idle_Rail_Call]`. Kamera od wieży nad las: pochodnie — **ze
wszystkich stron naraz**, także od kręgu (lud Garroka). Pod murem: cała armia Korony
`[A:Idle_Shield_Loop ×N]`. Noc (gameplay: obrona, z Koroną na murach — nie da się przegrać).
Świt — tableau: pole przed fortem, ciała `[A:Death01 ×N]`, żołnierze zbierają
`[A:Walk_Carry_Loop]`. Rynek: pomnik „SER GODRIC HARROW” `[C]`. Crane w łańcuchu gubernatora
`[A:Idle_Talking_Loop]` do tłumu — bez napisów, nie słyszymy go. Maud z boku.

> MAUD (V.O., bez ironii): They came. All of them. The law said they had to. *(pauza)* Pip rings at
> every dusk. Nobody comes. He says that's what she's for now. *(pauza)* The Commissioner is
> Governor. There's a statue. It's taller than Wat was.

Karta końcowa: *„The bell rang 4XX times. You ordered 4X of them.”* (`rings_total` /
`rings_ordered`, master §6.2.) Pod nią, mniejszym: *„Kell is a Crown colony.”*

#### Epilog BREAK (2 min + dwufazowa noc)

Maud i Pip z młotami `[C: prop]`, `[A:Interact]` ×3, dzwon pęka `[C: mesh split + dźwięk bez
pogłosu]`. **Cisza** — gra wycisza nawet ambient na 3 s.

Noc, faza 1: Skarn (boss, `warlord.rs`). Lud Garroka **po naszej stronie** (szamanka na murze), Idris
jeśli żyje. Świt, faza 2: Korona od strony zatoki — karna ekspedycja. Crane w tyle, nie walczy.

Jeśli gracz przetrwa — tableau: kuźnia, kowal `[A:Interact]`, **lemiesz z brązu** `[C]`. Pusty grób
przy bramie: stary napis zatarty, nowy `[C: decal]`: **„WAT. FOOTMAN.”** Pip bez dzwonu — stoi
na wieży `[A:Idle_Rail_Loop]`, patrzy na las. Nie dzwoni. Szamanka i ludzie Idrisa przy bramie
`[A:Idle_Loop]`, nie razem, ale obok.

> MAUD (V.O.): He laid three stones. We laid the rest.
> *(pauza)*
> MAUD (V.O.): The Commissioner's ships went east. He'll write it up as fever. Someone will read
> it in a hundred years and know better. *(pauza)* That's the most we get.

Karta końcowa: *„The bell rang 4XX times. You ordered 4X of them.”* Pod nią: *„Kell.”* Bez
„Crown colony”.

---

## 3b. Mapa ziaren: co gracz ma myśleć zamiast prawdy

| Ziarno (prawda) | Gdzie | O czym jest scena *naprawdę* dla gracza | Fałszywe wyjaśnienie (kto je mówi) |
|---|---|---|---|
| dzwon okrętowy → orki na plaży | P.1 | katastrofa, śmierć Harrowa, zbroja | „wyspa pełna orków” (nikt nie pyta) |
| cicha noc bez dzwonu | Rozdz. 3 | gorączka Pipa, Maud łamie rozkaz | **„rozbiliście rytuał szamanów”** (osadnik; staje się teorią wsi) |
| Hamid źle znosi zmierzch | Rozdz. 4 | niewola, orki śpiewają | „ból głowy po bębnach” (Hamid) |
| orki atakują karawany Pashy | Rozdz. 5 | konflikt z Idrisem, Tessarine | **„Korona przywiozła wojnę”** (Idris — i ma rację co do skutku) |
| inskrypcja na szkicu = na dzwonie | Rozdz. 6 | Anselm nie lubił Gubernatora; gorączka | „the Hours, monks” (Maud) |
| „came only after we rang” | Rozdz. 6 | j.w. | zalane, „rang the Hours” |
| edykt „bell at dusk” | Rozdz. 7 | dziesięcina (ta boli), godzina policyjna | trzeci z trzech, rutyna |
| pijany dzwon → atak na obóz Korony | Rozdz. 7 | dyscyplina Crane'a | „hałas ściągnął ich z lasu” (sierżant) — technicznie prawda |
| Crane: „fever” | Rozdz. 7 | pierwsza kolonia | strony Anselma **potwierdzają** gorączkę |
| orki nie atakują wieży | cała gra (AI) | — | nikt nie zauważa; w Rozdz. 10 Maud: „They never touched the tower.” |
| pochodnie/bębny po dzwonie | Rozdz. 1 | wieś zajmuje mury | — (wizualne, niekomentowane) |

Trzy konkurencyjne teorie gracza do Rozdz. 10: **rytuał** (wieś), **Korona przyniosła wojnę**
(Idris), **Pasha** (po spichlerzu). Każda jest częściowo prawdziwa, żadna nie jest o dzwonie. Garrok
nie obala ich — on dodaje czwartą, która wyjaśnia trzy poprzednie.

## 4. Bank rozmów T1 — ogniska i barki per akt

Master §6.4: barki *od żartów przez zmęczenie do ciszy*. Każdy akt ma własną pulę; kilka na start.

**Akt I (kpina — Maud odciąga uwagę od hełmu):**
- MAUD: „Ser Godric inspected the wall. The wall is inspected. The wall is also still a hole.”
- SETTLER: „Does he sleep in it?” MAUD: „Would you take it off, with what's out there?”
- PIP: „The well's clean. I did the well. Nobody says anything about the well.”
- WAT: „Ser Godric is not a carpenter.” MAUD: „Ser Godric isn't a lot of things. Hold the plank.”

**Akt II (zmęczenie; żarty rzadsze, celniejsze):**
- HAMID: „In the desert we have a saying about men in helmets. It doesn't translate. It isn't
  kind.”
- MAUD: „Tithe's a third now. Edict says it's for our protection. From what, it doesn't say.”
- SETTLER: „Six dead, and the Commissioner sent *grain*.” MAUD: „He sent grain and thirty men.
  Count both.”
- CRANE: „The Crown does not interfere with a Reeve's counting.”

**Akt III (cisza zaczyna się od Rozdz. 10 — napisy orków robią robotę):**
- ORK (mijany pod murem, napisy): „Not tonight. The iron voice again.”
- MAUD: „Nobody's laughing at Ser Godric any more. I miss it. It was useful.”
- HAMID: „I don't hum any more. Funny. Now that you'd understand it.”

**Akt IV (bez żartów):**
- SETTLER: „Will it be tonight?” MAUD: „Yes.”
- PIP: „Do you want her heart back?” MAUD: „Not yet.”
- SHAMANESS (napisy): „Widow. The little ones ask if you ring. I tell them I don't know. It is
  true.”

---

## 5. Budżet animacji per scena

| Scena | `[A]` gotowe | `[B]` Mixamo | `[C]` nasze |
|---|---|---|---|
| P.0 Pokład | Sitting_Idle, Fixing_Kneeling, PickUp_Table, Idle_Lantern, Sitting_Talking, LayToIdle | — | kajuta + międzypokład (wnętrze statku), hełm/list propy |
| P.1 Piorun | Rail_Call, Crouch_Idle, Sprint ×6, Idle_Torch, Death01 (Harrow), Sword_Idle | — | **4 stopklatki z piorunem** (flash → czerń), dłonie na pasku (ten sam klip co P.0 i Rozdz. 8) |
| P.2 Rozkazy | Sitting_Idle ×N, Talking, FoldArms | — | pergamin, czerwona koperta (propy dłoni) |
| P.3 Fort | Rail_Loop | — | wieża kamienna + dzwon z inskrypcją (decal) |
| 1 Pierwszy dzwon | Rail_Call, Sword_Idle | — | swing_bell (jest), pochodnie-potem-bębny timing |
| 1–4 ogniska | Sitting_*, Talking, FoldArms, LayToIdle, Sitting_Exit | Stumble (Hamid) | **naramiennik** (3-klatkowy klip dłoni — ziarno A) |
| 5 Parlament | Sitting_Idle/Exit, Talking, Shield_Loop | — | namiot, biała flaga |
| 6 Kaplica | PickUp_Table | — | karta pergaminu z rozmyciem, decal inskrypcji = ten sam co na dzwonie |
| 7 Statki | Walk_Formal, Shield_Loop ×8, Interact | Point, Look_Around | 3 statki, molo, skrzynie, dzwon okrętowy w piasku |
| 8 Najazd | Death01, Walk_Carry | Death02 | **spichlerz** (collapse + ogień), szabla prop |
| 8 Demaskowanie | Fixing_Kneeling, Sword_Attack | **Kneel** | **hełm zdejmowany** (swap + dłonie), pierwsza twarz |
| 9 Ognisko | Sitting_Talking | — | hełm prop obok |
| 10 Garrok | Sitting_Idle (orczy rig!), Idle_No, Sprint (młode) | Shrug | tron, miasto Hold (środowisko), **napisy pod warkot**, montaż wsteczny 4 stopklatek |
| 11a Krąg | Walk, Hit_Chest, Death01, Sprint, Fixing_Kneeling | Collapse | bełty, kusznicy w zaroślach, gong 3× |
| 11b Koperta | Idle ×N, Interact, FoldArms, Walk_Formal | — | koperta, serce dzwonu odczepiane |
| 11c Śmierć | Sword_Block, Hit_Knockback, Death01 | — | zasuwa bramy, kamera Maud, slow-mo bez wisp |
| 12 Ultimatum | Idle, Talking | — | biała chorągiewka |
| 12 Sojusznicy | Idle, PickUp_Table, Talking | — | — |
| 12 Wieża | Sitting_Idle, Rail_Call | — | serce dzwonu zmienia właściciela (cięcie) |
| RING | Rail_Call, Shield_Loop ×N, Death01 ×N, Walk_Carry, Talking | — | pomnik, łańcuch gubernatora, karta końcowa |
| BREAK | Interact ×3, Rail_Loop, Idle | — | **dzwon pęka** (split mesh), lemiesz, decal „WAT. FOOTMAN.”, młoty |

**Bilans:** `[B]` to **8 klipów** (Kneel, Shrug, Point, Look_Around, Collapse, Stumble, Death02 +
zapas Hands_Up) — jeden przebieg `bake_anims.py` + re-export 14 NPC + ork rig dla Garroka
(Sitting_Idle). Wszystkie `[B]` mają fallback `[A]` w tekście, więc **kampania jest grywalna na
samym `[A]`** z gorszym aktorstwem. `[C]` to 11 propów + 4 efekty; najdroższe trzy: spichlerz,
hełm-swap z dłońmi, pękający dzwon.

Opcja: UAL Pro (~$10, CC0, ten sam rig, bez retargetu) może mieć Kneel/Shrug/Point — sprawdzić w
`quaternius.com/animviewer.html` zanim sięgniemy po Mixamo.

---

## 6. Co ten dokument zakłada z mastera i czego nie rusza

- Struktura, obsada, zwroty A/B/C, koperta, „Ser Godric is down” zamiast sukcesji, jedna
  oskryptowana sukcesja Wat→Maud, licznik `rings_total`/`rings_ordered`, tabu wieży, lokalne
  dzwony, Pip nie ginie, dwa wybory (Idris; RING/BREAK) — wszystko z mastera, bez zmian.
- Otwarte decyzje mastera §10 przyjęte tak: **2 wybory**, **śmierć Wata na stałe** (także w RING),
  nazwy robocze zostają do czasu, aż ktoś je zmieni w masterze.
- Istniejące VO (`campaign_*.ogg`, Sully) nie jest tu używane; master §6.5 je wycina. Linie z
  `audio/lines.rs` sprzeczne z fabułą (np. „The Warlord's down… It's finally over.”) do przeglądu
  osobno.

## 7. Następne kroki (kolejność z mastera §6.6, M0)

1. `cutscene.rs`: `SceneScript` (RON) → kolejka ujęć (kamera/aktor-klip/linia/hold), `Modal::Cutscene`,
   letterbox, skip, `subtitles::force`; harness `FOREST_SCENE=<id>` pod `FOREST_CLIP`.
2. Prototyp **P.1 „Piorun”** (4 stopklatki, zero dialogu, same `[A]` + flash) — najtańszy test, czy
   „klatka z pioruna” niesie; potem **P.2** jako test aktorstwa bez twarzy.
3. Dopiero potem `[B]` z Mixamo i propy `[C]`.

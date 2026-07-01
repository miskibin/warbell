# Kampania Warbell — Misja 1 „The First Toll" (design)

> Uwaga językowa: ten dokument jest **po polsku**, żeby był zrozumiały dla autora. Wszystkie
> teksty, które faktycznie pojawiają się **w grze**, zostają **po angielsku** (gra jest
> angielska — całe UI, questy, głosy VO). Przy każdej angielskiej kwestii jest tłumaczenie w
> nawiasie.

## 1. Problem, który to rozwiązuje

Gra jest „nudna po kilku nocach". Diagnoza (potwierdzona w kodzie):

- **`quest.rs` to samouczek, nie fabuła.** Łańcuch: zbierz drewno → zbuduj farmę → drwala →
  kamieniołom → otwórz War Table → przeżyj noc. Po onboardingu **kończy się** i zapada cisza.
  Nic nie ciągnie gracza dalej.
- **`vignettes.rs` to nieme scenki.** Ładne dioramy (rozbity obóz, wrak), ale bez słów, bez
  ciągu, bez konsekwencji — mijasz, dostajesz buff, koniec.
- **Endgame jest mechaniczny.** Wygrana = wyłom w Gnashfang Hold + zabij Warlorda. To *warunek*,
  nie *opowieść*. Nic nie narasta narracyjnie, więc noc 5 czuje się jak noc 2.

Gra ma **klimat gry eksploracyjnej** (biomy, scenki, landmarki) i **pętlę RTS** (fale, ekonomia),
ale brakuje **kręgosłupa fabularnego**, który nadaje sens kolejnym nocom.

## 2. Rozwiązanie: „Kampania Warbell" (kierunek A+B)

Uporządkowany łańcuch **Rozdziałów**. Każdy rozdział spina trzy rzeczy:

1. **Depesza (fabuła / „po co").** Nazwany głos mówi do gracza — karta + napis + głos VO.
2. **Wyróżniona noc (mechanika / część B).** Rozdział niesie `NightScript`, który `siege.rs`
   czyta i buduje z niego INNĄ noc (inny skład wroga / kierunek / event).
3. **Wypłata i eskalacja.** Przeżycie beatu pcha rozdział dalej, stawka rośnie ku Warlordowi.

Rozdziały to **kamienie milowe**, nie każda noc. Zwykłe noce wypełniają luki między nimi.

**Ten dokument projektuje tylko Misję 1** (plus minimalny szkielet, bez którego nie ruszy).
Reszta 8-rozdziałowego łuku — osobny spec (patrz §8).

## 3. Spójność z trailerem (to jest źródło prawdy dla tonu i obsady)

Trailer (`promo/trailer-script-v6.md`, `promo/trailer-transcript.md`) ustala świat, głos i ton.
Kampania MUSI być z nim spójna, więc:

### Obsada — 3 głosy, wszystkie już nagrane w `src/audio/lines.rs`

- **HERO** — sam sobie narratorem. Zmęczony weteran, sucho-ironiczny, mituje samego siebie
  (*„But mostly me. I was magnificent."* = „Ale głównie ja. Byłem wspaniały."). To ON mówi do
  gracza — **nie** żaden steward/kasztelan (wcześniejszy pomysł „Bram/Wróżka" jest **odrzucony**
  jako niespójny z trailerem).
- **VILLAGER** — deadpan, przekłuwa każdą chełpliwą linię bohatera
  (*„He laid three stones. Then he supervised."* = „Położył trzy kamienie. Potem nadzorował.").
- **ORK / WARLORD** — tępe, mroczno-śmieszne mamrotanie
  (*„Warlord say march tonight. Warlord say march every night."* = „Warlord mówi maszerować dziś.
  Warlord mówi maszerować co noc.").

### Ton — sucha komedia, nie serio-oblężenie

Bohater opowiada epicką wojnę; wieśniak przekłuwa ją nieglamurową prawdą. Orki są tępe i
mroczno-zabawne. Każda depesza kampanii ma trzymać ten ton.

### Trailer = mechaniki, które JUŻ istnieją w grze

Cała fabuła trailera to rzeczy grywalne dziś:

| Beat trailera | Mechanika w grze |
|---|---|
| Warbell bije co noc → obrona muru | `siege.rs` (fazy Prep/Wave) |
| Gnashfang Hold = źródło, Warlord dowodzi marszami | `ork_fortress.rs`, `warlord.rs` (endgame) |
| Obozy wojenne w polach, „burned them down one by one" | `camps.rs` (czyszczenie warbandu) |
| Jeńcy w klatkach przy ogniskach → uwolnij, walczą u boku | `camps.rs` + `villagers::camp_rescue` |

**Zysk:** skoro trzymamy się trailera, **głos Misji 1 składamy z ISTNIEJĄCYCH klipów** — patrz §5.
„Pełny głos" na Misję 1 jest więc prawie darmowy, nie czeka na nowe nagrania.

## 4. Misja 1 — „The First Toll" (Pierwszy Dzwon)

Cel dydaktyczny: nauczyć gracza **pętli kampanii** (depesza → wyróżniona noc → domknięcie) przy
minimum nowej mechaniki. Wychodzi wprost z samouczka, którego ostatni quest to *„Survive the
Night"*.

### Przebieg — 4 beaty spięte z istniejącymi fazami `siege.rs`

Model faz: `GamePhase::Prep` (dzień, budowanie) ↔ `GamePhase::Wave` (noc, oblężenie).
`wave_index` startuje od -1 (dzień 1); pierwsza noc = index 0; świt = krawędź `Wave→Prep`.

**Beat 1 — Dzień 1, Prep: depesza otwierająca.**
Odpala się RAZ, na pierwszej klatce Prep gdy gracz ma kontrolę (latch typu „first_prep").
Dostarczenie: karta (notice) + napis (subtitle) + sting + głos VO.

> **Relacja do samouczka:** Misja 1 pokrywa się z ostatnim questem samouczka *„Survive the
> Night"* (obie dotyczą nocy 0). To NIE jest konflikt — depesze/napisy kampanii lecą *nad*
> samouczkiem, a **pill kampanii jest ukryty dopóki działa pill samouczka** (samouczek kończy się
> na świcie nocy 0). Beat 4 domyka jednocześnie ostatni quest samouczka i Rozdz. 1; od Rozdz. 2
> pill kampanii przejmuje ekran. Patrz §9.

> **HERO** *(sucho, dostojnie):* „This island was quiet once. Then Gnashfang's lot came down off
> the Hold. Tonight the warbell rings for the first time — and I'll be on the wall to meet them."
> *(„Ta wyspa była kiedyś cicha. Potem zeszła z Hold zgraja Gnashfanga. Dziś warbell zabije po raz
> pierwszy — a ja będę na murze, by ich powitać.")*
>
> **VILLAGER** *(deadpan):* „He'll be *near* the wall. Laid three of its stones, too. Then he
> supervised." *(„Będzie *koło* muru. Położył trzy jego kamienie. Potem nadzorował.")*

Pill trackera (wygląd zżynany z `quest.rs`): **„Chapter 1: The First Toll — Survive the first toll
of the bell."** *(„Rozdział 1: Pierwszy Dzwon — Przeżyj pierwsze uderzenie dzwonu.")*

**Beat 2 — Zmierzch (`Prep→Wave` edge, noc 0): dzwon.**
Warbell bije (audio + krótki flash HUD), jedna linijka bohatera — i TU uzbraja się `NightScript`.

> **HERO:** „First bell. They're not celebrating — they're counting. To the gate."
> *(„Pierwszy dzwon. Oni nie świętują — oni liczą. Do bramy.")*

**Beat 3 — Noc 0: wyróżniona noc (mechanika, część B).**
`NightScript` Rozdz. 1 — łagodny (to pierwszy raz), ale WYRAŹNIE inny: **podchody z południa**.
Skład = głównie grunty + paru zwiadowców, umiarkowana liczba, łuk spawnu zawężony do południowej
bramy (gracz uczy się „pilnuj południa"). **Bez** specjalnego eventu (taran zostaje na Rozdz. 3).
`advance_on = NightSurvived`. W trakcie nocy tępy bark zapowiadający Warlorda z ciemności:

> **ORK** *(daleko, stłumione):* „Warlord say march tonight. Warlord say march every night."

**Beat 4 — Świt (`Wave→Prep` edge, przeżyto): domknięcie + advance 0→1.**
Bohater domyka beat, wypłata nagrody, `CampaignLog` przesuwa rozdział 0→1, ziarno zapowiedzi.

> **HERO** *(ponuro-zadowolony):* „Held. That was a scouting party — Gnashfang's just clearing his
> throat." *(„Utrzymane. To był tylko oddział zwiadowczy — Gnashfang dopiero chrząka.")*
>
> **VILLAGER:** „Magnificent, m'lord. Truly. ...Can we fix the gate now."
> *(„Wspaniale, panie. Naprawdę. ...Możemy teraz naprawić bramę.")*

Nagroda: `Reward { stone: +?, gold: +? }` (dokładne liczby do zbalansowania w planie).

**Porażka** (twierdza pada / bohater ginie) → istniejący `GameOver`. Postęp kampanii jedzie z
savem, więc Continue wraca do **nie-przesuniętego** Rozdz. 1 (spełnia zasadę persist+reset z
CLAUDE.md).

## 5. Mapowanie głosu na ISTNIEJĄCE klipy VO

Misja 1 nie wymaga nowych nagrań — tekst składamy z klipów, które już są. Jeśli któryś nie pasuje
1:1, używamy transkryptu jako napisu, a nagranie podmieniamy później (mechanizm `voice:
Option<klucz>` — patrz §6).

| Beat | Kwestia | Istniejący klip / źródło |
|---|---|---|
| 1 (hero) | „This island was quiet once…" | `assets/audio/vo/hero/trailer_quiet.ogg` |
| 1 (hero, opcjonalnie) | „…I was magnificent." | `assets/audio/vo/hero/trailer_magnificent.ogg` |
| 1 (villager) | „…Then he supervised." | villager `supervised` (trailer, `lines.rs`) |
| 2 (hero) | „…they're counting." | `trailer_bell.ogg` / hero „counting" (trailer) |
| 3 (ork) | „Warlord say march…" | ork `march` chatter (`audio/ork.rs`, trailer) |
| 4 (hero/villager) | domknięcie | nowe linie — na start tylko napis, głos dograny później |

> Zgodnie z regułą CLAUDE.md: **każda linia głosu niesie swój transkrypt w komentarzu kodu** przy
> miejscu, gdzie klip jest ładowany/kluczowany, plus trigger.

## 6. Architektura (reuse-heavy, mały dotyk istniejącego kodu)

### 6a. `crates/core::campaign` (nowy moduł, czysta logika)

Wzorowany na `core::quest` (parity-testowany, `serde`, zero-dep, `f64`).

```rust
/// Postęp kampanii. Jedzie z savem, reset na New Game. (Analog QuestLog.)
pub struct CampaignLog { pub chapter: usize, pub progress: f64 }

/// Statyczna definicja rozdziału (dane, nie stan).
pub struct Chapter {
    pub id: &'static str,
    pub title: &'static str,
    pub open:  Dispatch,          // depesza otwierająca (Beat 1)
    pub toll:  Option<Dispatch>,  // krótka linia o zmierzchu (Beat 2)
    pub close: Dispatch,          // depesza domykająca (Beat 4)
    pub night: NightScript,       // przepis na noc (Beat 3)
    pub advance: AdvanceOn,       // warunek przejścia dalej
    pub reward: Reward,           // wypłata za domknięcie
}

pub struct Dispatch {
    pub speaker: Speaker,         // Hero / Villager / Ork / Warlord
    pub text: &'static str,       // transkrypt — ZAWSZE pokazany jako napis
    pub voice: Option<&'static str>, // klucz klipu; None = tylko napis (głos dograny później)
}

pub struct NightScript {
    pub mix: &'static [(OrkVariant, u32)], // wagi składu; Ch1: grunt + trochę scout
    pub count_scale: f64,                  // mnożnik liczebności; Ch1: łagodny
    pub arc: SpawnArc,                     // kierunek/łuk spawnu; Ch1: South
    pub event: Option<SpecialEvent>,       // Ch1: None (Ram/Warden/Escort na później)
}

pub enum AdvanceOn { NightSurvived /*, ReachLandmark, KillWarden, EscortDone … */ }
pub enum SpawnArc  { All, South, East, West, North }
pub enum SpecialEvent { /* puste na Ch1; Ram/Warden/Escort/Bloodmoon w kolejnych rozdz. */ }

pub static CHAPTERS: &[Chapter] = &[ /* CHAPTER_1 */ ];
```

Metody na `CampaignLog`: `current() -> Option<&Chapter>`, `record(Signal) -> Option<usize>`
(zwraca index domkniętego rozdziału, jak `QuestLog::record`), `is_complete()`.

**Parity-testy** (`cargo test -p tileworld_core`): `record(NightSurvived)` przesuwa rozdział 0→1;
poza aktywnym warunkiem nic nie robi; `is_complete()` po ostatnim rozdziale.

### 6b. `src/campaign.rs` (nowy plugin Bevy — spina core ze światem)

Wzorowany na `src/quest.rs`. Odpowiada za:

- **`CampaignLogRes(core::CampaignLog)`** — Resource opakowujący core; init + reset na
  `OnExit(StartScreen)` / `OnExit(GameOver)` (jak `reset_quests`).
- **Detektory (gated `run_if(in_state(Modal::None))`):**
  - *first-prep latch* → Beat 1 (depesza otwierająca), raz.
  - `Prep→Wave` edge → Beat 2 (dzwon) + **uzbrojenie** `NightScript` do zasobu
    `ActiveNightScript` (patrz 6c).
  - `Wave→Prep` edge (przeżyto) → Beat 4 (domknięcie) + `record(NightSurvived)` + nagroda.
    (Ten sam edge, na którym już siedzą `autosave_on_dawn` i `detect_survive`.)
- **Dostarczanie depesz** — reuse `ui::notice::Notice` (karta), `subtitles.rs` (napis),
  `audio::director` / `AudioCue` (głos + sting).
- **Tracker pill** — reuse wyglądu z `quest.rs` (`drive_tracker`), pokazuje aktywny rozdział.
- **Restore** — reconcile `CampaignLog` z `GameLoaded` (jak `restore_quest_log`).

### 6c. `siege.rs` (jedyny dotyk istniejącej mechaniki)

Nowy zasób `ActiveNightScript(Option<NightScript>)`. Spawner fali w `siege.rs`, budując noc,
czyta go: jeśli `Some`, użyj składu/łuku/liczby z przepisu; jeśli `None`, **domyślne dotychczasowe
zachowanie** (żeby zwykłe, niescenariuszowe noce działały jak dziś). `campaign.rs` ustawia zasób na
`Prep→Wave` i czyści na `Wave→Prep`.

To jedyna zmiana w istniejącym pliku gameplayowym — reszta to nowe moduły.

### 6d. `savegame.rs` (round-trip postępu)

Zgodnie z checklistą save z CLAUDE.md:

1. Dodaj `campaign: Option<CampaignLog>` do `SaveData` (`#[serde(default)]` — stare save'y ładują
   się jako `None`).
2. Zapis w `SaveCtx::snapshot()`; odczyt w `apply_pending_load`.
3. `campaign.rs` reconcile z `GameLoaded` (`None` = save sprzed kampanii → potraktuj jak brak
   rozpoczętej kampanii / rozdział 0, analogicznie do `restore_quest_log`).

Reset: New Game zeruje `CampaignLogRes` w `OnExit(StartScreen)` / `OnExit(GameOver)`.

## 7. Testy / weryfikacja

- **Core:** parity-testy `record()` / advance (§6a).
- **Ręcznie w grze:** zagraj Dzień 1 → sprawdź Beat 1 (karta+napis+głos), Beat 2 na zmierzchu,
  Beat 3 (orki z południa), Beat 4 na świcie + nagroda + pill znika.
- **Save:** zapisz w Prep po Rozdz. 1, wczytaj → rozdział = 1. Padnij podczas Rozdz. 1, Continue →
  rozdział wciąż 1 (nie-przesunięty).
- **Screenshot harness:** `FOREST_PANEL`/`FOREST_WAVE` do sfilmowania depeszy i wyróżnionej nocy
  (dokładne hooki dopnie plan implementacji).

## 8. Poza zakresem tej misji (świadomie)

- Rozdziały 2–8 (łuk mapujący 3 akty trailera: A „zbudowaliśmy to" → B „wróg: Hold/Warlord/bębny"
  → C „wojna: spalone obozy, uwolnieni jeńcy" → Warlord). Osobny spec, gdy Misja 1 udowodni pętlę.
- Specjalne eventy nocne (`Ram`, `Warden`, `Escort`, `Bloodmoon`) — `enum SpecialEvent` jest
  przygotowany, ale pusty na Ch1.
- Nowe nagrania VO — Misja 1 jedzie na istniejących klipach + napisach; głos dograny per-beat.
- Wpięcie `vignettes.rs` w spine (Wróżka zapowiada beaty) — opcjonalne, później.

## 9. Ryzyka

- **„Pełny głos od razu"** — złagodzone przez `voice: Option<klucz>`: tekst działa od dnia 1, głos
  wpina się gdy `.ogg` gotowy. Feature nie jest zablokowany na budce lektorskiej.
- **Dotyk `siege.rs`** — trzymamy go minimalnym (jeden zasób `ActiveNightScript`, `None` = stare
  zachowanie), żeby nie ruszać zbalansowanej mechaniki fali.
- **Zderzenie z `quest.rs`** — samouczek (`quest.rs`) i kampania (`campaign.rs`) to osobne systemy
  działające *równolegle* na nocy 0. Rozwiązanie (§4, Beat 1): depesze/napisy kampanii lecą nad
  samouczkiem, ale **pill kampanii jest ukryty, dopóki aktywny jest pill samouczka** (czyli do
  świtu nocy 0, gdy samouczek się kończy). Dzięki temu dwa trackery nigdy nie stoją naraz. Od
  Rozdz. 2 pill kampanii jest jedynym na ekranie. Dokładne warunki widoczności — do dopięcia w
  planie implementacji.

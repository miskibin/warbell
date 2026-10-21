# ElevenLabs — P.0 kajuta, retake

Tekst do nagrania cutscenki `FOREST_SCENE=cabin`. Zastępuje kwestie użyte w tym filmie
(`a1_harrow_01`–`04`, oraz pożyczki `a1_harrow_07`, `a1_harrow_08`, `a1_wat_01`, `a1_wat_05`).
Starych klipów nie nadpisywać — grają też poza tą sceną. Nowe pliki:
`assets/audio/vo/campaign/p0_<postać>_NN.ogg`.

Scena w kodzie (`src/scenes/cabin.rs`) nadal stoi na starych klipach. Podmiana po nagraniu.

## Po co retake

Harrow ma 50 lat, głos niski, **znudzony**. Mówi do Wata jak do mebla. Jedno zdanie o mieście,
bez nazwy. Pointa, która zostaje: „That's the point of you.”

Stary tekst tego nie trzyma.

- „You've done that strap every day for fifteen years and it's still wrong.” jest werdyktem.
  Model czyta to podniośle albo z żalem. Prośba o dociągnięcie paska nie ma skąd wziąć patosu.
  Zdanie jeszcze kłóci się z pointą: Wat wychodzi na człowieka, który od piętnastu lat nie umie
  jednej sprzączki, a chwilę później słyszy, że od tego właśnie jest.
- Dalej lecą osobne tematy: westchnięty list, uraza do Korony za wschodnią kampanię, zagadka
  „a wiesz, co o mnie mówili”, suchy dowcip „ja też nie”, teza o tarczy, „stop hovering”.
  Rekwizyt w kadrze nie skleja tematu, którego nie ma w zdaniach.

Nowy tekst ma jeden przedmiot. Wat dopina zbroję i wynosi list do ludzi, których słychać pod
podłogą. Harrow mówi tylko, kim ci ludzie są i po co Wat klęczy. Miasto jest jednym zdaniem
w środku tej myśli, nie nową sceną.

Piętnastu lat przy pasku tu nie ma. Widać je na obrazie (Wat zna sprzączkę), a padają później,
w jego własnej kwestii. „Today, if the sea allows.” i „after the eastern campaign.” wypadają:
jedno jest westchnięciem kapitana, drugie mową człowieka urażonego przydziałem.

## Ustawienia

Model: **Eleven v3**. Głosy te same co w banku (`VOICES.md`).

| | |
|---|---|
| Harrow | **Julian** — Deep, Rich and Mature |
| Wat | **Sully** — Mature, Deep and Intriguing |
| Stability | **Natural**. Suwak 0–1: `0.5`. Nie Creative — nuda schodzi w westchnienie. |
| Similarity | `0.70` |
| Speed | `1.05`, jak reszta banku. Wolniej ten głos robi się uroczysty. |
| Style exaggeration | `0`, jeśli suwak jest. |

Każdy blok poniżej to **osobna generacja**. v3 na dłuższym tekście buduje łuk i na końcu
podnosi zdanie. Pauzy między kwestiami robi montaż, nie model. W tekst nie wpisywać didaskaliów
— zostaną przeczytane.

Wyrzucić take, w którym Harrow wzdycha, zwalnia na „burned”, brzmi szlachetnie, żałośnie albo
czule. „Tighter.” ma paść od razu, bez nabierania powietrza pod kwestię.

## Kwestie

Mówiony tekst, już bez tagów. W tej kolejności.

> HARROW: Tighter.
> WAT: There.
> HARROW: Letter's on the table. Take it down to the Reeve.
> *(Wat bierze list, idzie do drzwi, staje — hymn przez pokład)*
> HARROW: Forty peasants and a Reeve.
> HARROW: Nobody looks at the man behind the shield, Wat.
> HARROW: They didn't look in the city, before it burned.
> HARROW: That's the point of you.
> WAT: Yes, Ser.
> HARROW: Stop hovering.

Cztery kwestie Harrowa od „Forty…” do „…point of you.” to jedna myśl. W montażu prawie bez
dziury, ten sam poziom, bez pioruna na „burned” i bez wejścia muzyki. Hymn zostaje pod spodem,
stłumiony deskami. Partytura w tych miejscach milczy (`cutscene-music-suno.md`, A1-1).

## Do wklejenia

### `p0_harrow_01` — Julian

```text
[flat] Tighter.
```

Jak ktoś prostuje przedmiot, nie jak ktoś prosi. Jeśli take jest miły, drugi przebieg:
`[flat] [bored] Tighter.` — i tylko wtedy.

### `p0_wat_01` — Sully

```text
[exhales] There.
```

Oddech po dociągnięciu sprzączki, nie stęknięcie pod zbroję. Bez `[pained]`, bez `[strained]`.

### `p0_harrow_02` — Julian

```text
[flat] Letter's on the table. Take it down to the Reeve.
```

Jedno polecenie. Kropka nie jest westchnięciem. Jeśli model robi między zdaniami teatr,
pociąć na dwa take'i i złożyć ciasno.

### `p0_harrow_03` — Julian

```text
[matter-of-fact] Forty peasants and a Reeve.
```

Liczy. Nie żali się. Nie `[bitter]`.

### `p0_harrow_04` — Julian

```text
[bored] Nobody looks at the man behind the shield, Wat.
```

### `p0_harrow_05` — Julian

```text
[flat] They didn't look in the city, before it burned.
```

To samo tempo i ta sama wysokość co poprzednia kwestia. „Burned” nie jest pointą zdania.
Jeśli głos na tym słowie siada — wyrzucić, nie „zostawić, bo ładne”.

### `p0_harrow_06` — Julian

```text
[bored] That's the point of you.
```

Nie ciszej i nie wolniej niż reszta. To ciąg dalszy, nie morał.

### `p0_wat_02` — Sully

```text
[quiet] Yes, Ser.
```

Przyjmuje polecenie. Nie pytanie, nie współczucie.

### `p0_harrow_07` — Julian

```text
[dismissive] Stop hovering.
```

Krótko. Nie `[angry]`, nie `[annoyed]` — oba dokładają powietrza albo warknięcia.
Wat odpowiedział i dalej stoi. Ma iść.

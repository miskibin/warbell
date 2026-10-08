# Situation-led first three days

The campaign introduces existing tools through a reason to use them. It does not award
resources for opening panels or completing an instruction. The payoff is a changed town
and a changed assault.

| Day | Situation | Action and visible consequence |
| --- | --- | --- |
| 1 | Villagers are held in a nearby camp. | K gathers the militia. The introductory camp has two Scouts. Defeating its guards opens the cage; freed people walk home and join the town. Return, stand down and build a farm. The next hint requires actual food production by a posted worker. |
| 2 | Torch bearers threaten the farms. | Buy a permanent upgrade at the War Table, then defend the fields. Opening the table does not complete the lesson. Arsonists visibly carry torches and prioritize farms over other producers. Dawn reports whether fields survived and explains rebuilding when needed. |
| 3 | Shamans are staging the next assault. | The compass identifies their camp. Clearing it removes every Shaman from this night's wave, without replacement. Alternatively, buy actual ranged support and face the full assault. The report shows the raid's result during preparation and battle. |

The first three preparation days have no countdown and stay in clear daylight. E at the war bell always begins the
night, even if the suggested action is unfinished. Early actions count, solo rescues count,
and combat advice replaces any unfinished preparation advice immediately. The first farm
trial also pauses a negative starvation meter until food production or the first night.
First-tier Keep Archers cost 80 gold, so normal income from the first two assaults can fund
this response even after the suggested Town Guard Arms purchase.

The tracker gives one reason and one short action. A named gold compass target supplies
bearing and distance, remaining visible at the edge when behind the camera. J opens optional
details; H remains the controls reference. After the third night, the detailed tracker
disappears. The ordinary preparation clock and threat report remain, with the staging camp
marked for independent raids.

## Night problems and daytime raids

The opening order is scouts, farm raiders, shamans, then a two-front attack. Later nights
rotate mixed formations: breakers, arsonists, casters and flanking Scouts. Table scaling is
bounded at 14 planned slots, 1.5× base HP and 1.3× damage before the existing difficulty and
hero-level modifiers.

A cleared staging camp suppresses only its announced specialists. Ordinary attackers
remain. Suppressed slots are consumed by the director, so a reduced wave still finishes.
Raid credit lasts through the promised night; normal respawn cannot cancel it. At dawn,
staging camps may regroup away from the hero. Holding an already defeated camp through
dawn also prevents its regrouping for that day.

Campaign facts and the per-day camp ledger are saved. Restore runs before simulation, and
camp entities reconcile against the saved ledger. A save between the last guard's death and
the rescue update can still free the cage after loading. Saves predating the campaign field
continue with their normal preparation clock and do not restart the introduction.

## Reproducible checks

```bash
cargo test -p tileworld_core --features serde
cargo test --bin tileworld_bevy_forest
cargo build
```

Run each engine scenario in an isolated data directory because ordinary autosaves remain
active:

```bash
BEVY_ASSET_ROOT="$PWD" \
XDG_DATA_HOME="$(mktemp -d)" \
FOREST_CAMPAIGN_VERIFY=raid \
FOREST_CAMPAIGN_VERIFY_HEADLESS=1 \
./target/debug/tileworld_bevy_forest
```

Replace `raid` with `defend` or `skip` for the other routes. Success logs
`CAMPAIGN_VERIFY SUCCESS` and exits successfully; failed assertions exit with an error.
Without the headless flag the scenario runs with the normal renderer.

These checks drive genuine K/B/E input, cage rescue, returning workers, building, food flow,
upgrade affordability/effects, wave spawning and dawn. Travel and combat outcomes are
scripted. Both prepared routes fund their purchases from normal kill rewards and tithe;
the ranged route fails if that income cannot pay for its suggested alternative.
The harness verifies system connections, not human navigation or combat balance.

For a normal gameplay-camera UI capture, use the existing `FOREST_SHOT` harness with
`FOREST_TPS=1`, `BEVY_ASSET_ROOT="$PWD"` and an isolated data directory. On memory-limited
software-rendering machines, `FOREST_RENDER_SYNC=1` serializes shader pipeline compilation
and bounds the general mesh slabs at 64 MiB to reduce upload peaks.
It does not alter normal rendering unless explicitly set.
For a capture after genuine New Game resets and its starting stipend, pair
`FOREST_CAMPAIGN_VERIFY=raid` with `FOREST_CAMPAIGN_VERIFY_HOLD=1`; this stops the script at
the first ready day while leaving the ordinary world running.

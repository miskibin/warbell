"""Build a portable evidence report from final, uncapped A/B runs and real game shots."""
import csv
import html
import json
from pathlib import Path
import shutil
import statistics

ROOT = Path(__file__).resolve().parents[1]
BASE = ROOT / "target/tree-study"
OUT = ROOT / "docs/studies/trees-2026-10-08"
NAMES = {"forest_close": "Las z bliska", "forest_wide": "Szeroki widok lasu", "siege": "Walka: 96 przeciwników", "low": "Las / Low (1 para)"}

def main():
    OUT.mkdir(parents=True, exist_ok=True)
    all_runs = json.loads((BASE / "summary.json").read_text(encoding="utf-8"))
    runs = [r for r in all_runs if r["label"].startswith("uncapped-")]
    if len(runs) != 20 or any(r.get("exit_code") != 0 or r.get("errors") for r in runs):
        raise RuntimeError("Expected 20 valid final uncapped runs")
    if any(r["performance"]["mode"] != "gameplay" or r["gpu_sample_count"] < 4 for r in runs):
        raise RuntimeError("Missing active gameplay or post-warmup GPU samples")
    if len({r["exe_sha256"] for r in runs}) != 1:
        raise RuntimeError("A/B executable changed")
    if len({json.dumps(r["tree_asset_sha256"], sort_keys=True) for r in runs}) != 1:
        raise RuntimeError("Tree assets changed during A/B")
    rows = []
    for scene in NAMES:
        def group(mode):
            return [r for r in runs if r["label"].startswith(f"uncapped-{scene}-{mode}-")]
        a, b = group("base"), group("new")
        n = 1 if scene == "low" else 3
        assert len(a) == len(b) == n
        values = {}
        for key in ("mean_ms", "p95_ms", "p99_ms"):
            for label, group_runs in (("base", a), ("new", b)):
                v = [r["performance"][key] for r in group_runs]
                values[f"{label}_{key}"] = statistics.median(v)
                values[f"{label}_{key}_range"] = [min(v), max(v)]
        for label, group_runs in (("base", a), ("new", b)):
            gpu = [r["gpu_sum_ms"] for r in group_runs]
            values[f"{label}_gpu_ms"] = statistics.median(gpu)
            values[f"{label}_gpu_ms_range"] = [min(gpu), max(gpu)]
            values[f"{label}_rss_gib"] = statistics.median(r["reported_rss_gib"] for r in group_runs)
        for key in ("mean_ms", "p95_ms", "gpu_ms"):
            values[f"delta_{key}_percent"] = 100 * (values[f"new_{key}"] / values[f"base_{key}"] - 1)
        rows.append({"scene": scene, "repetitions": n, **values,
                     "performance_gate": all(values[f"delta_{k}_percent"] <= 10 for k in ("mean_ms", "p95_ms", "gpu_ms"))})
    source = {"main": "90f8d1b1b4b780b481046e76a82c3d931b64d60b", "exe_sha256": runs[0]["exe_sha256"],
              "tree_asset_sha256": runs[0]["tree_asset_sha256"], "comparisons": rows, "runs": runs}
    (OUT / "results.json").write_text(json.dumps(source, indent=2), encoding="utf-8")
    with (OUT / "results.csv").open("w", newline="", encoding="utf-8") as f:
        writer = csv.DictWriter(f, fieldnames=[k for k in rows[0] if not k.endswith("_range")])
        writer.writeheader()
        writer.writerows({k: v for k, v in r.items() if not k.endswith("_range")} for r in rows)
    # Keep the underlying textual evidence, not just a summary table.
    logs = OUT / "logs"
    logs.mkdir(exist_ok=True)
    for name in ("uncapped-build.log", "core-tests.log"):
        shutil.copyfile(BASE / name, logs / name)
    for r in runs:
        for name in ("run.json", "stderr.log", "stdout.log"):
            shutil.copyfile(BASE / r["label"] / name, logs / f'{r["label"]}-{name}')
    for scene in ("trees", "forest_close", "forest_wide"):
        for mode in ("base", "new"):
            shutil.copyfile(BASE / f"report-{scene}-{mode}" / "shot.png", OUT / f"{scene}-{mode}.png")
    shutil.copyfile(ROOT / "art/blender_trees/studio_preview.png", OUT / "studio.png")
    table = ""
    ranges = ""
    for r in rows:
        delta = r["delta_mean_ms_percent"]
        gpu_delta = r["delta_gpu_ms_percent"]
        table += f'<tr><td>{NAMES[r["scene"]]}<small>{r["repetitions"]} × A/B</small></td><td>{1000/r["base_mean_ms"]:.1f} → {1000/r["new_mean_ms"]:.1f}</td><td>{r["base_mean_ms"]:.2f} → {r["new_mean_ms"]:.2f}<small>{delta:+.1f}% czasu klatki</small></td><td>{r["base_p95_ms"]:.2f} → {r["new_p95_ms"]:.2f}</td><td>{r["base_gpu_ms"]:.2f} → {r["new_gpu_ms"]:.2f}<small>{gpu_delta:+.1f}% sumy etapów</small></td><td>{"≤10%" if r["performance_gate"] else "Próg przekroczony"}</td></tr>'
        def span(key):
            lo, hi = r[key]
            return f"{lo:.2f}–{hi:.2f}"
        ranges += f'<tr><td>{NAMES[r["scene"]]}</td><td>{span("base_mean_ms_range")} → {span("new_mean_ms_range")}</td><td>{span("base_p95_ms_range")} → {span("new_p95_ms_range")}</td><td>{r["base_p99_ms"]:.2f} → {r["new_p99_ms"]:.2f}</td><td>{span("base_gpu_ms_range")} → {span("new_gpu_ms_range")}</td><td>{r["base_rss_gib"]:.3f} → {r["new_rss_gib"]:.3f}</td></tr>'
    forest_pass = all(r["performance_gate"] for r in rows if r["scene"] in ("forest_close", "forest_wide"))
    verdict = "Koszt mieści się w przyjętym budżecie." if forest_pass else "Koszt przekracza przyjęty próg w co najmniej jednym widoku lasu."
    page = '''<!doctype html><html lang="pl"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Warbell — studium drzew</title>
<style>*{box-sizing:border-box}body{margin:0;background:#101913;color:#ecede6;font:16px/1.6 system-ui,sans-serif}main{max-width:1280px;margin:auto;padding:52px 28px}header{max-width:870px}.eyebrow{letter-spacing:.2em;text-transform:uppercase;color:#b2c48a;font-size:12px}h1{font:500 clamp(38px,6vw,76px)/1.08 Georgia,serif;margin:18px 0}h2{font:500 30px/1.2 Georgia,serif;margin:44px 0 16px}p{color:#cbd2c9;max-width:930px}.lead{font-size:21px}.chips{display:flex;gap:8px;flex-wrap:wrap;margin:24px 0}.chips span{padding:7px 12px;background:#233027;border-radius:30px;font-size:13px}a{color:#d2dfaa}button{border:1px solid #4a5d4b;background:transparent;color:inherit;border-radius:6px;padding:9px 18px;font:inherit;cursor:pointer}button.active{background:#c3d49e;color:#142019;border-color:#c3d49e}.tabs{display:flex;gap:8px;flex-wrap:wrap;margin-bottom:12px}.compare{position:relative;width:100%;aspect-ratio:16/9;overflow:hidden;background:#263229;border-radius:10px}.compare img{position:absolute;width:100%;height:100%;object-fit:cover}.new{clip-path:inset(0 0 0 var(--split,50%))}.line{position:absolute;top:0;bottom:0;left:var(--split,50%);width:2px;background:#f8faf3}.tag{position:absolute;top:15px;left:15px;background:#132017dc;padding:5px 10px;border-radius:5px;font-size:13px}.tag.right{left:auto;right:15px}input{width:100%;accent-color:#c3d49e;margin:14px 0}.note{font-size:13px;color:#9fac9c}.card{background:#1c2920;border:1px solid #354636;border-radius:10px;padding:20px 24px;margin:24px 0}.scroll{overflow:auto}table{width:100%;border-collapse:collapse;font-size:14px}th,td{text-align:left;padding:15px 12px;border-bottom:1px solid #354636;white-space:nowrap}th{font-size:12px;text-transform:uppercase;letter-spacing:.08em;color:#adc193}small{display:block;font-size:12px;color:#a7b3a4}ul{padding-left:22px;color:#cbd2c9}details{padding:16px 0;border-bottom:1px solid #354636}summary{cursor:pointer}.studio{width:100%;border-radius:10px;margin-top:12px}footer{margin-top:48px;padding-top:20px;border-top:1px solid #354636;font-size:13px;color:#99a695}code{overflow-wrap:anywhere;font-size:12px}@media(max-width:600px){main{padding:30px 16px}th,td{padding:12px 9px}.lead{font-size:18px}}</style>
<main><header><div class="eyebrow">Warbell / Studium wykonalności / 08.10.2026</div><h1>Naturalniejsze drzewa.<br>Zmierzony koszt.</h1><p class="lead">Pięć modeli wykonanych przez subagenta Sol w Blenderze przez MCP, zaimportowanych do prawdziwej sceny gry i porównanych z drzewami z aktualnego maina.</p><div class="chips"><span>RTX 5060 Ti · 16 GB</span><span>i5-12400F</span><span>1920 × 1080</span><span>Release · High</span><span>118 podmienionych drzew</span></div></header>
<h2>Porównaj w grze</h2><div class="tabs"><button class="active" data-scene="trees">Modele z bliska</button><button data-scene="forest_close">Las</button><button data-scene="forest_wide">Szerszy widok</button></div><div class="compare" id="compare"><img id="before" src="trees-base.png" alt="Oryginalne drzewa"><img class="new" id="after" src="trees-new.png" alt="Drzewa z Blendera"><div class="line"></div><div class="tag">Oryginalne</div><div class="tag right">Blender MCP + Sol</div></div><label class="note" for="split">Przesuń granicę porównania</label><input id="split" type="range" min="0" max="100" value="50"><p class="note">To zrzuty z Warbell, nie render Blendera. Kamera i pora dnia są takie same; wiatr, postacie i inne animacje mogą mieć różne fazy.</p>
<div class="card"><strong>__VERDICT__</strong><p>Pipeline jest wykonalny: modele i tekstury działają w grze, a podmiana pozostaje opcjonalna. Z bliska gałęzie i sosna wyglądają naturalniej; z daleka korony są cieńsze i ciemniejsze. Finalna akceptacja wyglądu wymaga oceny całości sceny, w tym nadal stylizowanego terenu i krzewów.</p></div>
<h2>Wyniki po usunięciu limitu okna w tle</h2><p>Wartości są medianą z trzech 60-sekundowych uruchomień na wariant. Low to dodatkowa pojedyncza para. FPS = 1000 / średni czas klatki. Mniejsza wartość w ms jest lepsza.</p><div class="scroll"><table><thead><tr><th>Scena</th><th>FPS: przed → po</th><th>Średnia [ms]</th><th>p95 [ms]</th><th>GPU Σ [ms]</th><th>Próg kosztu</th></tr></thead><tbody>__TABLE__</tbody></table></div>
<p class="note">Próg ustalony przed pomiarami: ≤10% wzrostu średniej, p95 oraz sumy etapów GPU. GPU Σ jest sumą zarejestrowanych etapów, a nie niezależnym pełnym czasem GPU; etapy mogą się nakładać. Wyniki nie opisują wydajności laptopów ani kart zintegrowanych.</p>
<details><summary>Metoda i ograniczenia</summary><ul><li>Ta sama binarka A/B; przełącznik <code>FOREST_BLENDERTREES=1</code>. Identyczna kamera, jakość, rozdzielczość i oświetlenie. VSync wyłączony, pętla Bevy działa ciągle także bez fokusu.</li><li>15 sekund rozgrzewki od WorldReady; aktywna symulacja. Ustawienia i zapisy gracza odizolowane przez APPDATA. Brak renderowania Blendera i kompilacji podczas serii.</li><li>Wstępne testy wykryły niezależny limit 60 Hz nieaktywnego okna Bevy. Wyniki <code>main-natural-*</code> i <code>ab-*</code> są diagnostyczne, wykluczone z powyższej tabeli.</li><li>118 podmian w kampanii: 112 drzew leśnych i 6 przy osadzie. Drzewa martwe, pniaki, sad z owocami i roślinność pozostałych biomów zachowują obecne modele. RTS pominięty zgodnie z zakresem.</li><li>Brak osobnych poziomów geometrii LOD. Wspólna tekstura ma mipmapy; ich alfa korzysta z heurystyki, kora pozostaje nieprzezroczysta. Materiał jest współdzielony przez pień i liście.</li><li>Próba trwa minutę na uruchomienie; nie zastępuje wielogodzinnego testu stabilności ani szczegółowego profilowania systemów CPU.</li></ul></details>
<details><summary>Modele, walidacja i odtwarzanie</summary><p>Dwa dęby, dwie brzozy i sosna: 1170–1668 trójkątów/model, jeden atlas 1024×1024. Pliki GLB i JSON zgadzają się w atrybutach do 6e-7. Pięć GLB: 0 błędów i 0 ostrzeżeń walidatora Khronos. Testy logiki: 325 jednostkowych + 1 integracyjny przeszły.</p><p>Edytowalne źródło: <a href="../../../art/blender_trees/warbell_trees.blend">warbell_trees.blend</a>. <a href="../../../art/blender_trees/README.md">Instrukcja generowania i licencje</a>. Testy odtwarza <code>python tools/tree_study_suite.py</code>; instrukcja protokołu jest w <code>docs/tree-study-protocol.md</code>.</p><p>Interaktywną grę uruchamia <code>scripts/preview-blender-trees.ps1</code>, a wariant oryginalny ten sam skrypt z <code>-OriginalTrees</code>.</p></details>
<h2>Źródło studyjne</h2><p class="note">Ten obraz pochodzi z Blendera i pokazuje geometrię oraz atlas w neutralnym oświetleniu. Nie służył do pomiaru FPS.</p><img class="studio" src="studio.png" alt="Pięć modeli w neutralnym studio Blendera"><footer>Main: <code>90f8d1b</code> · Sterownik NVIDIA 616.64 · <a href="results.json">Dane i hashe JSON</a> · <a href="results.csv">Tabela CSV</a><p>Pełne logi każdej próby są w katalogu logs. SHA-256 binarki: <code>__HASH__</code></p></footer></main>
<script>const box=document.querySelector('#compare');document.querySelector('#split').addEventListener('input',e=>box.style.setProperty('--split',e.target.value+'%'));for(const b of document.querySelectorAll('[data-scene]'))b.addEventListener('click',()=>{document.querySelectorAll('[data-scene]').forEach(x=>x.classList.toggle('active',x===b));document.querySelector('#before').src=b.dataset.scene+'-base.png';document.querySelector('#after').src=b.dataset.scene+'-new.png';});</script></html>'''
    variability = '<div class="card"><strong>Różnica FPS wymaga ostrożnej interpretacji.</strong><p>Czas klatki zmieniał się znacznie między uruchomieniami, podczas gdy czasy GPU pozostawały stabilniejsze. W szczególności mediany zbliżenia lasu nie izolują wpływu drzew od zmienności pozostałej pracy systemu. Nie traktujemy procentowej zmiany FPS jako ustalonego kosztu samych drzew. Decyzję o pozostawieniu eksperymentu uzasadnia powtarzalny wzrost GPU.</p></div><details><summary>Zakresy prób, p99 i pamięć</summary><p class="note">Minimum–maksimum między uruchomieniami; p99 i RSS są medianami wyników prób. Wszystkie czasy w ms. RSS to przybliżony odczyt istniejącego monitora, nie dokładny pomiar pamięci samych drzew.</p><div class="scroll"><table><thead><tr><th>Scena</th><th>Zakres średniej: przed → po</th><th>Zakres p95</th><th>p99</th><th>Zakres GPU Σ</th><th>RSS [GiB]</th></tr></thead><tbody>'+ranges+'</tbody></table></div></details>'
    page = page.replace("<details><summary>Metoda i ograniczenia", variability + "<details><summary>Metoda i ograniczenia")
    page = page.replace("__TABLE__", table).replace("__VERDICT__", html.escape(verdict)).replace("__HASH__", runs[0]["exe_sha256"])
    (OUT / "index.html").write_text(page, encoding="utf-8")
    print(json.dumps(rows, indent=2))

if __name__ == "__main__":
    main()

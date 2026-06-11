# Generic per-scene encoder: stitch a FOREST_CLIP frame dir into an mp4 with a layered SFX mix.
# NO music here — music + titles are added by build_trailer.ps1 in post.
# Copy this into promo/ and edit the $bed + $ev tables per scene. Run from repo root.
#   powershell -File promo/encode_scene.ps1 -frames target/clips/<dir> -out promo/<scene>.mp4
param(
  [string]$frames = "target/clips/scene",
  [string]$out    = "promo/scene.mp4",
  [int]$fps       = 30,
  [int]$maxframes = 0          # 0 = use all frames; else cap (trim a clip that overruns the action)
)
$ErrorActionPreference = 'Stop'
$A = "assets/audio"

# ── looping ambience beds: @{f=path; v=gain} ────────────────────────────────────────
# e.g. forest-ambient for outdoors; wind + campfire-loop for a night siege.
$bed = @(
  @{f="$A/forest-ambient.ogg"; v=0.45}
)

# ── one-shot SFX events on a timeline: @{f=path; t=seconds; v=gain} ──────────────────
# Times are PLAYBACK seconds (the encode is 1:1; only the in-engine SCRIPTING uses the
# frame-locked ClipProgress clock — see the skill's timing gotcha).
$ev = @(
  # @{f="$A/chop-wood-1.ogg"; t=0.9; v=0.85},
  # @{f="$A/sword-swing.ogg"; t=1.0; v=0.75}, @{f="$A/sword-hit-1.ogg"; t=1.18; v=0.9},
)

# ── build the filtergraph ───────────────────────────────────────────────────────────
$inputs = @("-framerate", "$fps", "-i", "$frames/frame_%05d.png")
$fc = ""; $mix = ""
$idx = 1
foreach ($b in $bed) {
  $inputs += @("-stream_loop", "-1", "-i", $b.f)
  $fc += "[${idx}:a]volume=$($b.v)[bed$idx];"
  $mix += "[bed$idx]"
  $idx++
}
foreach ($e in $ev) {
  $inputs += @("-i", $e.f)
  $ms = [int]($e.t * 1000)
  $fc += "[${idx}:a]volume=$($e.v),adelay=${ms}|${ms}[s$idx];"
  $mix += "[s$idx]"
  $idx++
}
$n = $bed.Count + $ev.Count
# normalize=0 is mandatory — amix's default divides every stem by the input count (see skill §4).
$fc += "${mix}amix=inputs=${n}:normalize=0,alimiter=limit=0.9[aout]"

$vframes = if ($maxframes -gt 0) { @("-frames:v", "$maxframes") } else { @() }
ffmpeg -y @inputs -filter_complex $fc -map "0:v" -map "[aout]" @vframes `
  -c:v libx264 -preset slow -crf 19 -pix_fmt yuv420p -c:a aac -b:a 192k -shortest $out

ffprobe -v error -show_entries format=duration -of csv=p=0 $out
ffmpeg -i $out -af volumedetect -f null NUL 2>&1 | Select-String 'mean_volume|max_volume'

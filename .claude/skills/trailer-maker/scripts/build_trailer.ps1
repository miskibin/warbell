# Rebuilds the 6-scene trailer from the per-scene mp4s (SFX-only clips; music + titles added here).
# Order: explore -> build -> work -> talk -> rescue -> defend, 0.5s xfade between scenes.
# Music: music-bed (day) under scenes 1-5, crossfading into the LOUD section of
# soot-banner-dread (night siege track) which carries the defend scene to the end.
# Mixes use amix normalize=0 everywhere — default amix normalization is what buried the
# night music in the previous build (each stem got divided by the input count).
# Run from repo root:  powershell -File promo/build_trailer.ps1
$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot

$clips = @('explore','build','work','talk','rescue','defend')
$XF = 0.5   # crossfade seconds

# ── probe durations ──────────────────────────────────────────────────────────
$dur = @{}
foreach ($c in $clips) {
  $dur[$c] = [double](ffprobe -v error -show_entries format=duration -of csv=p=0 "$c.mp4")
}
# cumulative video timeline with xfade (each scene starts at prev_total - XF)
$start = @{}; $t = 0.0
foreach ($c in $clips) { $start[$c] = $t; $t += $dur[$c] - $XF }
$total = $t + $XF
Write-Host "scene starts: $($clips | ForEach-Object { '{0}={1:n1}' -f $_, $start[$_] }) total=$('{0:n1}' -f $total)"

# ── video chain: xfade + per-scene titles ────────────────────────────────────
$font = ($PWD.Path -replace '\\','/') + '/../assets/fonts/EBGaramond.ttf'
$font = $font -replace '^(\w):','$1\:'   # ffmpeg filter args: bare drive colon ends the option
$titles = @{
  explore = 'Explore the Wilds';   build  = 'Forge a Stronghold'
  work    = 'Tend the Realm';      talk   = 'Mind the Locals'
  rescue  = 'Free the Captured';   defend = 'Hold the Night'
}
$inputs = $clips | ForEach-Object { "-i", "$_.mp4" }
$fv = ""
$prev = "[0:v]"
for ($i = 1; $i -lt $clips.Count; $i++) {
  $off = '{0:n3}' -f ($start[$clips[$i]])
  $out = if ($i -eq $clips.Count - 1) { "[vx]" } else { "[v$i]" }
  $fv += "$prev[${i}:v]xfade=transition=fade:duration=${XF}:offset=$off$out;"
  $prev = "[v$i]"
}
# titles: fade in/out, lower third, per-scene window
$draw = ""
foreach ($c in $clips) {
  $t0 = $start[$c] + 0.8; $t1 = $t0 + 3.2
  $alpha = "if(lt(t,$t0),0,if(lt(t,$($t0+0.5)),(t-$t0)/0.5,if(lt(t,$($t1-0.5)),1,if(lt(t,$t1),($t1-t)/0.5,0))))"
  $txt = $titles[$c]
  $draw += ",drawtext=fontfile='$font':text='$txt':fontsize=54:fontcolor=white:alpha='$alpha':x=(w-text_w)/2:y=h-150:shadowcolor=black@0.7:shadowx=2:shadowy=2"
}
$fv += "[vx]format=yuv420p$draw[vout]"

# ── audio: SFX chain (acrossfade mirrors the video xfade) ────────────────────
$fa = ""
$prevA = "[0:a]"
for ($i = 1; $i -lt $clips.Count; $i++) {
  $out = if ($i -eq $clips.Count - 1) { "[ax]" } else { "[a$i]" }
  $fa += "$prevA[${i}:a]acrossfade=d=${XF}$out;"
  $prevA = "[a$i]"
}

# ── music stem ───────────────────────────────────────────────────────────────
# bed: 0 .. defend_start+1.0 (fade out 2.5s into the night track's rise)
# night: soot-banner-dread loudest stretch (~69.5s in), enters 1.5s before the defend cut,
#        fades in 1.2s, plays to total, 2s tail fade.
$dStart  = $start['defend']
$bedEnd  = $dStart + 1.0
$nIn     = $dStart - 1.5
$nLen    = $total - $nIn
$musIdx  = $clips.Count       # next input indices
$nightIdx = $clips.Count + 1
$inputs += @('-i', '../assets/audio/music-bed.ogg', '-i', '../assets/audio/soot-banner-dread.ogg')
$fm  = "[${musIdx}:a]atrim=0:$('{0:n2}' -f $bedEnd),volume=0.55,afade=t=in:d=1.5,afade=t=out:st=$('{0:n2}' -f ($bedEnd-2.5)):d=2.5,apad=whole_dur=$('{0:n2}' -f $total)[mbed];"
$fm += "[${nightIdx}:a]atrim=69.5:$('{0:n2}' -f (69.5+$nLen)),asetpts=PTS-STARTPTS,volume=1.0,afade=t=in:d=1.2,afade=t=out:st=$('{0:n2}' -f ($nLen-2.0)):d=2.0,adelay=$([int]($nIn*1000))|$([int]($nIn*1000)),apad=whole_dur=$('{0:n2}' -f $total)[mnight];"
$fm += "[mbed][mnight]amix=inputs=2:normalize=0[music];"
$fm += "[ax]apad=whole_dur=$('{0:n2}' -f $total)[sfx];"
$fm += "[sfx][music]amix=inputs=2:normalize=0,alimiter=limit=0.92[aout]"

$filter = "$fv;$fa$fm"
ffmpeg -y @inputs -filter_complex $filter -map "[vout]" -map "[aout]" `
  -c:v libx264 -preset slow -crf 19 -c:a aac -b:a 192k -t $total tileworld-trailer.mp4

# ── verify: the music stem must be audible over the last scene ───────────────
Write-Host "`n--- last-scene loudness (must be well above -30dB mean) ---"
ffmpeg -ss ($total - 10) -i tileworld-trailer.mp4 -af volumedetect -f null NUL 2>&1 | Select-String 'mean_volume|max_volume'

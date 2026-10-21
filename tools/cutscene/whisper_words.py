import sys, json
from faster_whisper import WhisperModel
m = WhisperModel("large-v3", device="cpu", compute_type="int8", cpu_threads=8)
segs, info = m.transcribe(sys.argv[1], language="en", word_timestamps=True, vad_filter=False)
out=[]
for s in segs:
    out.append({"s":s.start,"e":s.end,"t":s.text,"w":[(w.word,round(w.start,2),round(w.end,2)) for w in s.words]})
json.dump(out, open(sys.argv[2],"w"), indent=1)
for s in out: print(f"{s['s']:.2f}-{s['e']:.2f} {s['t']}")

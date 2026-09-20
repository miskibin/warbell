#!/usr/bin/env python3
"""Build and launch a normal, instrumented play session. Python stdlib only."""
import argparse
import datetime
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--seconds', type=float, default=1800, help='recording limit; game stays open afterwards')
    parser.add_argument('--no-build', action='store_true', help='use already built executable')
    parser.add_argument('--output', type=Path, help='new recording directory (default profiles/timestamp)')
    parser.add_argument('--stacks', action='store_true', help='macOS: also sample native CPU stacks during active gameplay')
    args = parser.parse_args()
    if not 1 <= args.seconds <= 7200:
        parser.error('--seconds must be between 1 and 7200')
    processes = subprocess.run(['ps', '-axo', 'pid=,comm='], capture_output=True, text=True)
    for row in processes.stdout.splitlines():
        fields = row.strip().split(None, 1)
        if len(fields) == 2 and Path(fields[1]).name == 'tileworld_bevy_forest':
            parser.error('Warbell is already running. Save and close it before starting a recording.')
    env = os.environ.copy()
    env['PATH'] = str(Path.home() / '.cargo/bin') + os.pathsep + env.get('PATH', '')
    cargo = shutil.which('cargo', path=env['PATH'])
    if not args.no_build:
        if not cargo:
            parser.error('Rust cargo was not found')
        subprocess.run([cargo, 'build', '--locked'], cwd=ROOT, env=env, check=True)
    binary = ROOT / 'target/debug/tileworld_bevy_forest'
    if not binary.is_file():
        parser.error('Game executable is missing; run without --no-build')
    stamp = datetime.datetime.now().strftime('%Y%m%d-%H%M%S-%f')
    output = (args.output or ROOT / 'profiles' / stamp).expanduser().resolve()
    output.mkdir(parents=True, exist_ok=False)
    # Avoid accidentally changing gameplay through inherited test/demo hooks.
    for key in list(env):
        if key.startswith('FOREST_'):
            del env[key]
    env.update(FOREST_PROFILE=str(output), FOREST_PROFILE_SECONDS=str(args.seconds),
               BEVY_ASSET_ROOT=str(ROOT), RUST_LOG='warn,tileworld_bevy_forest=info')
    revision = subprocess.run(['git', 'rev-parse', 'HEAD'], cwd=ROOT, capture_output=True, text=True).stdout.strip()
    diff = subprocess.run(['git', 'diff', '--binary'], cwd=ROOT, capture_output=True).stdout
    (output / 'source.patch').write_bytes(diff)
    # Include untracked profiler tools/source so a local experiment is reproducible before commit.
    tracked = subprocess.run(['git', 'ls-files', '--others', '--exclude-standard', 'src', 'tools'], cwd=ROOT, capture_output=True, text=True).stdout.splitlines()
    for relative in tracked:
        path = ROOT / relative
        if path.is_file() and path.suffix in ('.rs', '.py', '.md'):
            destination = output / 'untracked-source' / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(path, destination)
    (output / 'launch.json').write_text(json.dumps({'revision':revision, 'platform':platform.platform(),
        'processor':platform.processor(), 'logical_cpus':os.cpu_count(), 'seconds':args.seconds,
        'stacks':args.stacks, 'binary':str(binary), 'binary_mtime':binary.stat().st_mtime,
        'started':datetime.datetime.now().astimezone().isoformat()}, indent=2))
    print(f'Recording directory: {output}', flush=True)
    print('Play normally. F8 marks a slow moment. Loading, pauses and background time are separated in the report.', flush=True)
    print('Close the game to finish, or let the recording limit expire; the game can remain open.', flush=True)
    start = time.monotonic()
    stack_job = None
    stack_file = None
    active_since = None
    last_stack = -120
    last_report = 0
    reader = None
    latest_frame = None
    capture_stopped = False
    with (output / 'game.log').open('w') as log, (output / 'system.jsonl').open('w', buffering=1) as system:
        process = subprocess.Popen([str(binary)], cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT)
        (output / 'pid.txt').write_text(str(process.pid))
        try:
            while process.poll() is None:
                now = time.monotonic() - start
                capture = output / 'gameplay.jsonl'
                if reader is None and capture.exists():
                    reader = capture.open()
                if reader:
                    while True:
                        pos = reader.tell()
                        line = reader.readline()
                        if not line:
                            break
                        if not line.endswith('\n'):
                            reader.seek(pos)
                            break
                        try:
                            record = json.loads(line)
                        except json.JSONDecodeError:
                            continue
                        if record.get('type') == 'frame':
                            latest_frame = record
                            active = (record.get('app_state') == 'Playing' and record.get('modal') in (None, 'None')
                                      and record.get('focused') and record.get('world_ready'))
                            active_since = (active_since if active_since is not None else now) if active else None
                        elif record.get('type') == 'end':
                            capture_stopped = True
                if now <= args.seconds + 5:
                    result = subprocess.run(['ps', '-p', str(process.pid), '-o', '%cpu=,rss='], capture_output=True, text=True)
                    parts = result.stdout.split()
                    if len(parts) == 2:
                        system.write(json.dumps({'type':'process','t_s':now,'cpu_percent':float(parts[0]),'rss_kib':int(parts[1]),
                                                 'system_load_average':os.getloadavg() if hasattr(os,'getloadavg') else None})+'\n')
                    # Native sampling gives full-engine stacks without a separate Bevy trace build.
                    # It adds overhead; the sidecar explicitly records the sampling interval.
                    if (args.stacks and platform.system() == 'Darwin' and stack_job is None
                            and active_since is not None and now-active_since >= 10 and now-last_stack >= 120):
                        stack_path = output / f'cpu-stacks-{int(now):06d}.txt'
                        stack_file = (output / f'cpu-stacks-{int(now):06d}.log').open('w')
                        stack_job = subprocess.Popen(['/usr/bin/sample',str(process.pid),'5','10','-file',str(stack_path)], stdout=stack_file, stderr=subprocess.STDOUT)
                        system.write(json.dumps({'type':'stack_sample_begin','t_s':now,'path':stack_path.name})+'\n')
                        last_stack = now
                if stack_job is not None and stack_job.poll() is not None:
                    system.write(json.dumps({'type':'stack_sample_end','t_s':now,'exit_code':stack_job.returncode})+'\n')
                    stack_file.close()
                    stack_job = None
                if capture.exists() and now-last_report >= 30:
                    subprocess.run([os.sys.executable,str(ROOT/'tools/analyze_gameplay.py'),str(capture),
                                    '--output',str(output/'report.md'),'--json',str(output/'summary.json')], stdout=subprocess.DEVNULL)
                    last_report = now
                if capture_stopped:
                    print(f'Capture finished. Game remains open. Report: {output / "report.md"}', flush=True)
                    # Finish any in-flight sampler before returning control; leave the game alone.
                    break
                time.sleep(2)
        except KeyboardInterrupt:
            print('Launcher interrupted; game was not terminated.', flush=True)
        finally:
            if reader:
                reader.close()
            if stack_job:
                stack_job.wait()
                stack_file.close()
            if (output / 'gameplay.jsonl').exists():
                subprocess.run([os.sys.executable,str(ROOT/'tools/analyze_gameplay.py'),str(output/'gameplay.jsonl'),
                                '--output',str(output/'report.md'),'--json',str(output/'summary.json')], check=False)
    print(f'Report: {output / "report.md"}', flush=True)


if __name__ == '__main__':
    main()

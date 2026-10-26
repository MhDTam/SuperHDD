"""Analyze the captured HDD-only run; no changes to the game or cache."""
import csv
import json
import math
import statistics
from datetime import datetime
from pathlib import Path

workspace = Path(__file__).resolve().parent.parent
root = workspace / 'artifacts/benchmarks/euro-resources-20261023T133854Z-cedd63cf'
other = workspace / 'artifacts/benchmarks/euro-resources-20261023T134124Z-c512adc5'
with (root / 'frames.csv').open(encoding='utf-8-sig', newline='') as file:
    frames = list(csv.DictReader(file))
assert frames and {row['ProcessID'] for row in frames} == {'9588'}
assert len({row['SwapChainAddress'] for row in frames}) == 1
start = datetime.fromisoformat(frames[0]['TimeInDateTime'])
for row in frames:
    row['elapsed'] = (datetime.fromisoformat(row['TimeInDateTime']) - start).total_seconds()
usable = [row for row in frames if row['elapsed'] >= 3 and float(row['MsBetweenPresents']) > 0]
intervals = sorted(float(row['MsBetweenPresents']) for row in usable)
slowest_count = max(1, math.ceil(len(intervals) * .01))
resource_rows = []
for directory in [root, other]:
    meta = json.loads((directory / 'metadata.json').read_text(encoding='utf-8-sig'))
    assert meta['error'] is None and meta['gameId'] == 9588
    resource_rows.extend(json.loads(line) for line in (directory / 'samples.jsonl').read_text(encoding='utf-8-sig').splitlines())

def resource(key, factor=1):
    values = [row['values'][key] * factor for row in resource_rows if row['values'].get(key) is not None]
    return {'mean': statistics.mean(values), 'min': min(values), 'max': max(values)}

final_state = json.loads((root / 'cache-state-end.json').read_text(encoding='utf-8-sig'))
assert next(row for row in final_state['apps'] if row['id'] == '9be1bad80cbe5c8cebff4c1e977df6e5')['phase'] == 'stopped'
report = {
    'game': 'Euro Truck Simulator 2', 'gamePid': 9588, 'cacheMode': 'stopped, original HDD directory',
    'frameTool': 'PresentMon 2.6.0 portable console, ETW, no game injection',
    'captureSeconds': frames[-1]['elapsed'], 'capturedFrames': len(frames),
    'excludedInitialSeconds': 3, 'analyzedFrames': len(usable),
    'averagePresentedFPS': 1000 / statistics.mean(intervals),
    'onePercentLowFPS': 1000 / statistics.mean(intervals[-slowest_count:]),
    'onePercentLowDefinition': '1000 divided by the mean interval of the slowest ceil(1% of analyzed frames)',
    'frameTimeP50Ms': statistics.median(intervals),
    'frameTimeP95Ms': intervals[math.ceil(.95 * len(intervals)) - 1],
    'frameTimeP99Ms': intervals[math.ceil(.99 * len(intervals)) - 1],
    'maxFrameTimeMs': max(intervals),
    'framesOver33_333ms': sum(value > 1000/30 for value in intervals),
    'framesOver50ms': sum(value > 50 for value in intervals),
    'framesOver100ms': sum(value > 100 for value in intervals),
    'worstFrames': [{'elapsedSeconds': row['elapsed'], 'intervalMs': float(row['MsBetweenPresents']),
                     'gpuBusyMs': float(row['MsGPUBusy'])} for row in sorted(usable, key=lambda row: float(row['MsBetweenPresents']), reverse=True)[:12]],
    'resourceSamples': len(resource_rows),
    'gameCpuPercentOf16LogicalProcessors': resource('processCpuMachinePercent'),
    'availableMemoryGiB': resource('memoryAvailableMiB', 1/1024),
    'hddPhysicalReadMiBPerSecond': resource('sourceReadBytesPerSecond', 1/1048576),
    'hddPerSampleAverageReadLatencyMs': resource('sourceReadSeconds', 1000),
    'hddSampledQueueDepth': resource('sourceQueueDepth'),
    'limitations': [
        'An uncontrolled gameplay baseline. No matched cache-enabled game run yet.',
        'Present intervals are measured, not optical display latency. Event-loss diagnostics were not captured.',
        'CSV wall-clock timestamps show a six-hour offset relative to the launch UTC metadata; only frame-relative times are used.',
        'Display latency/flip delay values are implausible in this capture and are excluded.',
        'The two resource recordings have a gap and are not precisely correlated to individual frame spikes.',
        'Physical D disk counters include other processes and any pagefile I/O; process IO counters include logical reads.',
        'Total CPU percentage does not rule out a main-thread limit; page faults include soft faults.',
    ],
    'conclusion': 'Frame-time spikes occur with the application cache stopped. The cause and the additional effect of the new cache worker are not established.',
}
(root / 'analysis.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
summary = f'''# Euro Truck: HDD-only gameplay baseline

The application cache was stopped at the start and end. No folder worker was running at the end. The game process and its settings were preserved.

| Metric | Result |
| --- | --- |
| Capture | {report['captureSeconds']:.1f} seconds, {len(frames):,} frames |
| Average presented FPS | {report['averagePresentedFPS']:.1f} |
| 1% low (mean of slowest 1% intervals) | {report['onePercentLowFPS']:.1f} FPS |
| 99th percentile frame time | {report['frameTimeP99Ms']:.2f} ms |
| Worst frame time | {report['maxFrameTimeMs']:.2f} ms |
| Frames above 50 ms | {report['framesOver50ms']} |
| Frames above 100 ms | {report['framesOver100ms']} |

The first three seconds were excluded from summary statistics. Two large spikes occurred about 83.6 and 83.8 seconds into the capture, with intervals of roughly 214 and 207 ms. GPU busy time for those frames was roughly 5.2 ms; this does not identify the waiting cause.

These measurements establish a baseline with stutters even without the app cache. They do not establish why the stutters occurred or whether the new worker improves them. A comparable cache-enabled run is still required.

The JSON report includes resource summaries, raw file locations and measurement limitations. The CSV's absolute timestamps and display-latency fields are not used because they are inconsistent with the recording metadata.
'''
(root / 'summary.md').write_text(summary, encoding='utf-8')
print(json.dumps({key: report[key] for key in ['capturedFrames','averagePresentedFPS','onePercentLowFPS','maxFrameTimeMs','framesOver50ms','framesOver100ms','resourceSamples','availableMemoryGiB']}, indent=2))

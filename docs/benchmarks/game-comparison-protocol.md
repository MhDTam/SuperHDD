# Game loading comparison protocol

The random-read test is a storage microbenchmark, not a game loading benchmark.
Do not use chat response times or disk inactivity as loading endpoints.

1. Verify exact start/end events through an application log or captured frames
   of the same selected battle, characters, arena and settings. Record endpoint
   uncertainty; missing endpoints invalidate loading-time conclusions.
2. Collect original-path direct, proxy bypass, proxy empty and proxy prewarmed
   conditions, closing the game normally before directory restoration changes.
3. Separate OS-cold and OS-warm experiments. An empty SSD index does not make
   Windows' memory cache cold. Keep shader preparation and background activity
   comparable; do not silently purge memory or reboot the user's machine.
4. Record CPU, physical disk/file I/O, hard faults, cache counters and worker
   memory. Identify source D, cache M and trace-writing activity separately.
5. Ten paired repetitions per condition, balanced order, identical warm-up,
   medians/spread and paired uncertainty. Report failures and startup/miss costs.

`scripts/record-game-trace.ps1` collects bounded 10–120 second WPR memory-mode
resource traces using an administrator token and a uniquely owned instance.
It neither changes cache state nor injects game input. Exported resource traces
alone do not establish exact loading endpoints or a comparative speedup.

Current preflight: WPR/WPA/xperf are installed. Standard-user WPR start failed
with `0xc5585011` (failed to enable system performance profiling policy).
Computer Use discovered the game window, but both capture attempts failed:
`FrameArrived timed out: timed out waiting on channel`, then
`window capture timed out: timed out waiting on channel`. No input was injected
after those failures. No precise visual endpoint is currently available.

Official references:
- https://learn.microsoft.com/en-us/windows-hardware/test/wpt/recording-for-resource-based-analysis
- https://learn.microsoft.com/en-us/windows-hardware/test/wpt/wpr-command-line-options

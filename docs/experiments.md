# Measuring the split

Use a repeatable game benchmark, replay, or stationary scene. Fix game settings,
FPS cap, resolution, refresh rates, scaling, VRR, video, codec, and browser window
size across conditions. Record these settings in the notes. Warm shader caches
before collecting results.

1. Get the rotating nine-run schedule with `igpu-man experiment`.
2. Fully exit the browser before each GPU change. Launch through the appropriate
   profile and confirm requested placement against observed engine activity.
3. Allow the scheduled 60-second warm-up. Start a labelled recording and capture
   120 seconds of the same scene. Attach the matching MangoHud log.
4. Note playback quality and visible frame drops manually, including uncertainty.
5. Repeat all conditions three times. Compare recordings for median/p95/p99
   frametime, frames exceeding the configured threshold, GPU activity, power
   readings where available, and variation between repeated runs.

Average FPS is 1000 divided by mean frametime. Percentiles use nearest-rank over
the frame samples in each log; a threshold exceedance is a frame above the chosen
millisecond threshold, not a diagnosis of stutter. Separate files are shown
independently; use one matching log per experimental run. If a recording has
several imported logs, run variation uses its first log and shows a warning.
Run variation is a
min–max range, not a confidence interval. Averages of telemetry readings are
sample means, not energy integrals.

The comparison flags changes in recorded environment/monitor metadata. The
software cannot detect an unrecorded game-setting or video-content difference.
Profiles and launch preferences are retained separately because the GPU target
is intentionally different between conditions.

After a driver, kernel, or compositor update, repeat the same experiment.
Do not infer a regression from a single run. A result smaller than normal
run-to-run variation should be described as inconclusive. A quieter dGPU is
useful evidence of placement, but only game and playback measurements establish
whether the arrangement is better for you.

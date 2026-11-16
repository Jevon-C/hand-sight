# Finger-tapping metrics (draft)

Status: draft for clinical review. These are research measures, not clinical scores, and they are
not yet validated against clinician ratings. The definitions are fixed for each test version; a
change to any of them is a new version and gives new results.

## Signal

Opening = distance between the thumb tip and index tip, divided by the palm length (wrist to
middle-finger knuckle). Dividing by palm length makes the signal independent of how far the hand is
from the camera. Each tap is one rise and fall of this signal.

## Finding the taps

The signal is smoothed with a three-point average. The typical tapping period is found from its spectrum, and candidate peaks
(local maxima at least `min_amplitude` palm lengths high, default 0.15, that also close again:
they stand at least half of `min_amplitude` above the lower of their two sides) are kept largest
first, no closer than half that period. Closing again is what makes a tap: a hand held open with
small wobbles has none. When the video ends before one side of a peak has been seen, only the other
side is judged. The height of a tap is read from the unsmoothed signal next to the
smoothed peak. If fewer than the minimum number of taps are found, or too much of the fingertip
track is missing, the run fails with a plain explanation instead of giving unreliable numbers.

## Measures

| Id | Meaning | Unit |
| --- | --- | --- |
| `tap_count` | number of taps found | count |
| `tap_rate` | (taps − 1) divided by the time from first to last peak | taps/s |
| `mean_amplitude` | mean widest opening of a tap | palm lengths |
| `amplitude_variability` | standard deviation of the amplitudes divided by their mean, times 100 | % |
| `amplitude_decrement` | a straight line is fitted to the amplitudes in tap order; the fall from its first to its last value as a percentage of the first (positive = the taps shrink) | % |
| `hesitations` | gaps longer than `hesitation_factor` (default 1.5) times the median gap, but not longer than `arrest_factor` times it | count |
| `arrests` | gaps longer than `arrest_factor` (default 3) times the median gap | count |

## Quality checks

Each is `pass` or `flag` and is stored with the result: number of taps (fewer than 10 makes the
slowing measures unreliable), share of frames in which both fingertips were found (at least 95
percent), and length of tapping (ten seconds or more is advised).

## Known limits

At least 4 taps are needed. The stand-in hand model is
only for development, and its numbers are marked illustrative.

## Contact time (used by the evaluation)
Each closing of the fingers is a low point of the opening that stands at least half of
`min_amplitude` below its surroundings (the same rule as for taps, turned upside down). Its
contact is the first frame, after the preceding high point, at which the opening has fallen to
within 10% of the way down to the low point: the moment the fingertips touch. A closing from a
held-open hand counts as a contact; a closing the video ends before completing does not. It is
what a person marks in the evaluation (docs/EVALUATION.md).

## Version history
- 0.2.0 (2026-11-13): a tap must also close again (found on a real clip: a hand held open with
  small wobbles before the tapping began was counted as two taps). Results made with 0.1.0 differ.
- 0.1.0: first version.

## Literature check (2026-11-13)
Three papers on video-based finger tapping were read, partly through machine-extracted summaries
(the methods sections were not always available verbatim), so confirm any figure against the paper
before quoting it. A fourth page could not be read (it returned a bot check).

| Paper | Tracker, frame rate | What they define |
| --- | --- | --- |
| [Ehsan et al., arXiv 2506.18925](https://arxiv.org/abs/2506.18925) | MediaPipe, 25 fps | Thumb to index distance scaled by hand size; a tap is a peak, its amplitude is the peak minus its paired trough; an interruption is an interval longer than 2 times the median interval; the sequence effect is the slope of a straight-line fit |
| [Islam et al., arXiv 2303.17573](https://arxiv.org/abs/2303.17573) | MediaPipe, 30 fps | Amplitude as the thumb-wrist-index angle, chosen because a distance depends on the distance to the camera; interruptions and freezes defined by speed falling below a threshold |
| [Guarin et al., 2024, PMC11260436](https://pmc.ncbi.nlm.nih.gov/articles/PMC11260436) | MediaPipe, 30 fps | Thumb-index angle low-pass filtered at 7 Hz; peaks and valleys; amplitude decay as the mean of the first half divided by the mean of the second half; no hesitation or arrest definition |

**Where we agree:** the signal is a normalised thumb to index measure with peaks as taps; the rate
is taps over time; the amplitude decrement is a fitted slope (as in Ehsan et al.); cycle-to-cycle
variability is reported. A 3-point average at 30 frames per second behaves like a low-pass filter
near 7 Hz, as in Guarin et al.

**Where we differ, and what we do about it:**
- *Amplitude.* Ours is the widest opening (height above zero). Ehsan et al. use the peak minus its
  paired trough, which ignores a trough above zero when the fingers do not fully close. Both are
  defensible; ours is kept and named "widest opening". Revisit when clips rated by clinicians exist.
- *Arrests.* We call a gap longer than 3 times the median an arrest and 1.5 to 3 times a hesitation.
  The only precedent found is 2 times the median for an interruption (Ehsan et al.). Neither
  threshold has been validated. They stay settings of the test, and results record them.
- *Smoothing.* Ours depends on the frame rate (3 points), theirs is a fixed 7 Hz filter. A
  frequency-based filter is the better design; see the smoothing check in docs/PLAN.md.
- *Frame rate.* The papers used 25 to 30 frames per second. The capture requirement was 50; it is
  now 30 with 60 preferred, because two frames at 30 per second (67 ms) is the best timing a
  person marking by eye can give (docs/EVALUATION.md).
- *Not measured here:* tap speed (opening and closing), a measure all three papers rely on.

**What this means for the claims.** The papers report agreement with clinician ratings at the
level of about half the videos exactly right and 94% within one point (Ehsan et al.), and a mean
absolute error of 0.58 points against 0.53 between two expert raters (Islam et al.); the
clinicians themselves agree only moderately (Krippendorff's alpha 0.62 to 0.69). None of them
reports how closely the software finds each tap in time. Our evaluation measures that, not
agreement with clinicians. Until clinician-rated clips exist, nothing here is a clinical score.

## Smoothing study (2026-11-13)
The tap analysis smooths the opening signal with a 3-point average before looking for peaks. The
geometry core's constant-velocity Kalman filter with a backward pass (`smooth_track`) was compared
with it, and with no smoothing, on synthetic tapping with known truth (`python -m
axonhand.tools.smoothing_study`; 18 runs per row: rates 1, 2 and 3.5 per second, 30 and 60 frames
per second, 3 seeds). Noise is the standard deviation of each landmark's position in palm lengths.

| Noise | Smoother | Wrong tap count | Peak time error (ms) | Amplitude error (%) |
| --- | --- | --- | --- | --- |
| 0.00 | none, average, Kalman | 0 of 18 | 5.4 | 0.5 |
| 0.01 | none | 0 of 18 | 11.6 | 1.4 |
| 0.01 | 3-point average | 0 of 18 | 8.1 | 1.4 |
| 0.01 | Kalman | 0 of 18 | 6.9 | 1.4 |
| 0.02 | none | 0 of 18 | 14.1 | 2.9 |
| 0.02 | 3-point average | 0 of 18 | 11.1 | 2.9 |
| 0.02 | Kalman | 0 of 18 | 8.0 | 2.8 |
| 0.04 | none | 13 of 18 | 15.3 | 5.1 |
| 0.04 | 3-point average | 1 of 18 | 17.3 | 6.1 |
| 0.04 | Kalman | 1 of 18 | 9.2 | 5.7 |

Smoothing of some kind matters at high noise (without it 13 of 18 runs found the wrong number
of taps). The Kalman smoother gives tap times 25% to 45% closer to the truth at moderate and high
noise and is no worse elsewhere. Two cautions: it was given the true noise size, which a real run
does not have, and the synthetic noise is white while real tracking error comes in bursts. So the
3-point average stays the default. The next step, to be taken when real clips exist, is to
estimate the noise from the signal and compare the two on marked clips.


## The other three hand tests (2026-11-13, version 0.1.0 each)

These plug-ins (`hand_open_close`, `pronation_supination`, `postural_tremor`) were written after
the finger-tapping one and have had **less checking**: they are verified against synthetic hands
whose truth is known (tests/test_movements.py), and nothing else. They have not been compared
with the literature, with a person's marks or with a clinical rating, and they stay labelled
"Illustrative". All are research measures, not clinical scores.

### Opening and closing the hand (MDS-UPDRS 3.5)

Signal: the mean distance of the five fingertips from the wrist, in palm lengths (a fist is about
1, a spread hand about 1.9). A cycle is an opening found by the finger-tapping rule (a peak at
least `min_range` high, measured above the 5th percentile of the signal, that closes again by
half of that, at least half a period from its neighbours). The size of a cycle is its **range**:
the peak minus the low points on either side (for the first and last cycle only the side between
cycles, since a video may start or end with the hand held open). Measures: cycle count, cycle
rate ((n-1) over the time from first to last opening), mean range, range variability, range
decrement (straight-line fit, first to last), hesitations and arrests (gaps against the median
gap, factors 1.5 and 3 as for tapping). At least 4 cycles are needed.

### Pronation and supination (MDS-UPDRS 3.6)

Signal: the signed area of the triangle wrist, index knuckle, little-finger knuckle, divided by the
palm length squared. It is positive with one face of the hand towards the camera, negative with
the other and zero edge on. A **turn** is a passage through zero, counted when the signal goes
from beyond a dead band (a quarter of the 95th percentile of its size) on one side to beyond it on
the other, and placed by interpolation at the zero. Two turns are a full cycle. Measures: turn
count, cycle rate, turn size variability and decrement (size is the largest face-on value
between two turns, relative to the largest in the video), hesitations and arrests. At least 6
turns are needed.

Limits, stated plainly:
- The **angle** turned through is not measured. The face-on value rises as the sine of the angle
  from edge on and saturates near 90 degrees, so a falling swing shows as a smaller fall in
  turn size (in the synthetic test a 30 percent fall in angle shows as about 10 percent).
- The camera must see the faces of the hand: with the forearm pointing at the camera the palm stays
  edge on and the test cannot be measured (the plug-in says so). The capture instructions ask for
  the forearm upright in front of the chest.
- The landmark model must keep its landmark numbering when the back of the hand is shown; this
  has not been checked on real video.

### Postural tremor (MDS-UPDRS 3.15)

Signal: the position of the palm (mean of the wrist and the four finger bases, which move together)
in palm lengths, x and y. Welch's method (2 second Hann windows, half overlap, each detrended) gives
the power spectrum, x and y added. The tremor band is 3 to 12 Hz (lowered to 0.9 of half the frame
rate if needed). Measures: **frequency** (the strongest peak in the band, refined by a parabola
through three bins), **amplitude rms** (square root of the in-band power) and the peak to peak
size of a steady oscillation with that rms (2.83 times it), **band share** (in-band power as a
percentage of the power from 1 Hz up) and **regularity** (the share of in-band power within 1 Hz of
the peak). At least 5 seconds are needed (10 advised).

Limits, stated plainly:
- The hand model has jitter of its own. It is estimated from the spectrum above the band (needs
  a frame rate above about 28) and the plug-in flags a tremor that is under three times that noise
  level. The estimate assumes the noise is flat in frequency; this has not been measured on a
  hand held still.
- Amplitude is in palm lengths. Millimetres need the scale from a calibration board (not yet
  built), and a camera that moves adds movement that looks like tremor: use a stand.
- 30 frames per second sees tremors up to 12 Hz but gives a poor noise estimate; 60 is advised.
- The tremor of one hand is measured; the clinical item scores each hand and each posture
  separately, so run it once for each.

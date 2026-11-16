# Evaluation: how the error is measured

Phase 5 exit check: measure the error of the hand model and of each tapping measure on clips
marked by a person, and write it down. "Illustrative" labels stay on results until the
measurement supports removing them.

## What is measured, and against what

A person watches each clip and marks every tap at the frame where the thumb and index fingertips
**touch** as the fingers close (the first frame of contact). The marks are kept beside the video
as `<video>.taps.json`. Contact is used instead of the widest opening because it is sharp: on a
slow, flat opening two people (and the software) cannot agree on a "widest" frame, which was seen
on the first real clip (a 6-frame difference on one tap), while the closing is quick and clear.

The software finds the same moment from the opening signal: for each closing of the fingers, the
first frame after the preceding high point at which the opening has fallen to within 10% of the
way down to its low point (`axonhand.tapping.contact_frames`; docs/METRICS.md). Tap amplitude is still the widest opening;
only the timing used for the comparison is the contact.

From the marks we can judge: whether each tap was found (recall) and whether the software
invented any (precision); how far in time each found tap is from the marked one; and the measures
that depend only on tap times: tap count, tap rate, hesitations, arrests. The software and the
reference use the same gap rule (`axonhand.tapping.gap_counts`). A closing from a held-open hand
at the start counts; the closing of the very last tap is counted only if it was filmed.

We cannot judge amplitude, amplitude variability or decrement this way: a person cannot mark
"0.8 palm lengths" by eye. Those need a second reference (a printed ruler or a depth sensor
filmed together with the video) and are not covered yet. The report says so.

## Matching rule

A found tap is a hit if it is within half the typical marked gap of a marked tap (never less than
two frames): the widest tolerance that still leaves each tap one possible partner. Matching is
one-to-one, closest pairs first. How exact the hits are is judged separately by the timing
errors. (This was a quarter of the gap until the first real clip showed one tap 0.2 s apart
counted as a miss plus an extra; the change was made after seeing that result and is recorded
in D13.) Unmatched found taps are extra; unmatched marked taps are missed. A clip the software
cannot analyse counts as a failure and its taps count as missed.

## Proposed acceptance limits (to be confirmed, see D13)

At least 10 clips with none failing; recall and precision at least 95%; every clip's tap rate
within 5% of the marked rate; 95% of tap times within 50 ms of the marked time. Until a report
meets these on real clips, results from the real model keep the "Illustrative" flag. The timing limit is
tentative until two people have marked the same clips and their own spread is known. The limits are
a proposal: they were chosen to be tighter than the difference between clinical rating steps, not
taken from a standard, and should be checked against the literature (a separate Phase 5 item).

## How to run

1. Record clips (10 or more, different people if possible, some slow, some irregular, some with
   poor light) and put them in one folder.
2. For each clip: `python -m axonhand.tools.annotate path\to\clip.mp4`, press M at each tap,
   S to save. Ideally a second person marks a few of the same clips under another name (copy the
   clip to a second folder) so the person-to-person difference is known: the software cannot be
   judged more precisely than people agree with each other.
3. `python -m axonhand.evaluate path\to\folder` writes `data/eval/report.md` and `report.json`.
4. `python -m axonhand.compare_markers first\folder second\folder` compares two people's marks of
   the same clips and writes `data/eval/agreement.md`. The 95th percentile time difference there is
   the least a timing limit can sensibly be.

## Limits of this method

Marks are placed by eye at video frame rate, so timing error below about one or two frames is noise.
Phone video is variable frame rate in places; the evaluator reads frame times from the decoded
frames, as the analysis does. Clips filmed by the developer are not a clinical sample.

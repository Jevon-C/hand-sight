# Hand-landmark model assessment (2026-11-12)

Source: pages read on 2026-11-12 (listed at the end). Nothing here was run yet; no model file
has been downloaded or inspected. Items marked UNVERIFIED were not confirmed on a primary page.

## Candidates

| Candidate | Licence evidence | Output | Accuracy evidence | Status |
| --- | --- | --- | --- | --- |
| MediaPipe hand landmark (Google), 2021 model card | Apache 2.0 stated on the model card | 21 landmarks, screen 3D (z estimated from synthetic data) and metric world landmarks | MNAE (palm-size-normalised 2D error): Full 10.09%, Lite 12.02% on a 700-image geographic set; 5.08% and 5.67% on a 420-image skin-tone/gender set; human re-annotation 6.0% and 3.8% | Provisional primary |
| opencv_zoo ONNX conversion of MediaPipe hand pose (`..._2023feb.onnx`, 4.1 MB fp32) | README: all files Apache 2.0; LICENSE is stock Apache text with empty copyright line | 21 keypoints per detected hand; tensor shapes and z/handedness outputs UNVERIFIED | None stated; README warns the int8 file can lose a lot of accuracy | Best licence evidence for an ONNX file |
| Qualcomm AI Hub MediaPipe hand (detector + landmark, float ONNX/QNN/TFLite) | HF metadata says apache-2.0, but its LICENSE points to the upstream repo `zmurez/MediaPipePyTorch`, which was not read; card does not address commercial use | Landmark count and shapes UNVERIFIED | Latency only, no accuracy | Licence unresolved |
| RTMPose-m hand (OpenMMLab) | Code Apache 2.0; weights licence not stated; training-dataset licences not checked | 2D only | COCO-WholeBody-Hand val: PCK@0.2 0.815, AUC 0.837, EPE 4.51 px at 256x256 | Backup; 2D only, so no depth |

## What the model card says about limits (matters for us)
- "Trained on limited datasets", meant for experimental usage; primary use research and entertainment; not for life-critical decisions.
- Intended for prominently displayed hands in phone images. Out of scope: gloves, and occlusions such as hands holding objects.
- Not tested in low light, motion blur or low-end cameras. Evaluation sets come from the same source as training data.
- No clinical populations (tremor, rigidity, bradykinesia) are mentioned anywhere.
- Error is smaller at finger bases and larger toward fingertips, and larger for blurry or occluded joints.

## Consequences for the project
1. Finger tapping and open-close fit the model's intended use (open, prominent hand). Reach-to-grasp
   does not: the hand holds objects, which the card lists as out of scope. This is exactly where our
   temporal hand-state network and fine-tuning must earn their place, and it should be reported honestly.
2. A 10% palm-size error is large for tremor. If palm length is about 10 cm (my assumption), that is
   about 1 cm average, comparable to the tremor amplitude bands. Tremor needs measured error on our
   data and likely temporal smoothing; treat tremor as the hardest, last item.
3. Do not use the opencv_zoo int8 file as is. Quantise ourselves with real hand frames and report
   int8 versus fp32 landmark error.
4. Published numbers are on general phone images. Our accuracy claims must come from our own
   test: synthetic motion with known parameters (metric recovery), plus a small real set filmed
   with a calibration board and an independent reference.

## Next steps
1. Download the opencv_zoo fp32 ONNX files and the palm-detector branch (needs your permission),
   inspect input/output tensors, and run them through ONNX Runtime on the laptop CPU and NPU.
2. Read the upstream licence at zmurez/MediaPipePyTorch if the Qualcomm files are wanted.
3. Check the licences of the datasets behind any model we fine-tune on (FreiHAND, COCO-WholeBody, etc.).
4. Define the accuracy test (metric recovery on synthetic data; real-set protocol).

## Pages read
- Google MediaPipe hand tracking model card (Oct 2021), read through a mirror host, extracted text only.
- huggingface.co/opencv/handpose_estimation_mediapipe: README, file listing, LICENSE. The palm-detector branch and GitHub copy could not be read.
- huggingface.co/qualcomm/MediaPipe-Hand-Detection: card, listing, LICENSE. The GitHub directory and upstream licence were not read.
- github.com/open-mmlab/mmpose hand RTMPose README and detail page, LICENSE; github.com/Tau-J/rtmlib (partly).

## Measured on the laptop (2026-11-12)
Files from opencv_zoo (`models/palm_detection_mediapipe`, `models/handpose_estimation_mediapipe`),
Apache-2.0 licence files saved with them. Hashes are in `models/SOURCES.md` (not committed).
Timings use random input, so they show speed only, not accuracy. Median of 50 runs after warm-up,
float32 models, ONNX Runtime 1.31.1 with the Qualcomm provider 2.8.0.

| Model | Size | Input (NHWC) | Outputs | CPU ms | NPU ms | GPU ms |
| --- | --- | --- | --- | --- | --- | --- |
| Palm detector | 3.91 MB | 1 x 192 x 192 x 3 | 2016 anchors x 18 box values; 2016 scores | 9.12 | 1.18 | 3.16 |
| Hand landmarks | 4.10 MB | 1 x 224 x 224 x 3 | 63 (21 points x 3); hand score; handedness; 63 world points | 4.28 | 0.81 | 1.70 |

Reading the table:
- Both models run on the NPU about 5 to 8 times faster than on the CPU. One detector run and one
  landmark run together take about 2 ms on the NPU, far inside the 16.7 ms available per frame at 60 fps.
- The CPU 95th percentile is high (59 to 91 ms) because of occasional stalls; the NPU is steady (about 1.2 ms).
- "Providers in use" lists the CPU as a fallback for every NPU run, so some small operations may still
  run on the CPU. A per-node profile is still to do.
- Not yet known: accuracy on our videos, and the speed once frames are decoded and cropped.
- The landmark model gives 63 "world" values as well as image-space points, which may help with scale;
  how reliable they are is untested.

## Pipeline built (2026-11-13)
`axonhand.hand` implements the two-stage design with the two files above: letterbox to 192 x 192,
palm detection (2016 anchors, score threshold 0.5, overlap 0.3), a rotated square region (palm box
scaled 2.6, shifted 0.5 of its height toward the fingers; rotation from wrist to middle-finger
base), crop to 224 x 224, landmark model, and mapping back to the image. Later frames take their
region from the previous landmarks (scale 2.0, shift 0.1) and look for the palm again when the
landmark model's presence score falls under 0.5.

Tested without a hand: anchor layout, decoding, overlap removal, region rotation and size, the
crop mapping against Pillow, the tracker's logic with stand-in models, and that a plain or noisy
picture gives no hand with the real models. **Not yet tested: anything on a real hand.** The
constants above come from the published MediaPipe design and the opencv_zoo wrapper, which I did
not run side by side; the first real-video check is the contact sheet from
`python -m axonhand.tools.draw_landmarks`.

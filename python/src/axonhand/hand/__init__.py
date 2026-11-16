"""Real hand-landmark pipeline: palm detector, landmark model, tracking and video reading.

The geometry (``rois``) is plain numpy and fully tested without any model; ``runtime`` opens an
ONNX model on the NPU or the CPU; ``tracker`` ties them together for one frame at a time; and
``video`` runs it over a whole file and returns a :class:`axonhand.landmarks.LandmarkTrack`.
"""

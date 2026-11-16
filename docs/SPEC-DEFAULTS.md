# Working defaults for open spec items (adopted 2026-11-12)

Adopted as the working defaults when the user said "let's go". Numbers are starting proposals,
to be revisited after the first baseline. Principle: spend effort on the geometry core, the temporal
network with ablations and sim-to-real, and the edge benchmark; take the cheapest sound option elsewhere.

| # | Item | Default |
| --- | --- | --- |
| 1 | Primary success metrics | Finger-tapping rate, amplitude, decrement; synthetic recovery within 5%; about 30 fps on the laptop. Grasp metrics secondary, exploratory |
| 2 | First user | Clinician; pharma analyst API-only for now. Workflow: upload, quality gate, pick test, view results |
| 3 | Phase 1 tests | Finger tapping, open-close, reach-to-grasp. Next: pronation-supination. Stretch: tremor. ALS: stub plug-in only |
| 4 | Clinical sign-off | Metrics labelled research-grade, definitions from literature; seek one physio or neurologist reviewer |
| 5 | Capture protocol | One page: fixed camera on tripod, 1080p at 60 fps, plain background, calibration board in view, 10 s trials; failed videos kept and flagged with reason |
| 6 | Rust/Python boundary | Python plug-ins run as separate worker processes (JSON results, files for arrays); PyO3 only to expose the Rust geometry core to Python |
| 7 | Geometry targets | Reprojection error < 0.5 px synthetic, < 2 px real; metric scale error < 3% via the board; planar accuracy claimed for tapping, approximate 3D for grasp |
| 8 | Models | Temporal net 0.5 to 2 M parameters, < 10 MB int8; about 100,000 synthetic sequences, 200 to 500 real clips; detector starts with cup, ball, pen, fine-tuned from a pretrained torchvision model |
| 9 | Quantisation | int8 adds at most 0.5% of palm length to landmark error versus fp32; NPU fallback accepted, NPU node share reported |
| 10 | Plug-in manifest | JSON validated by JSON Schema in Rust and Python; scan `plugins/` at startup; semver; comparable results need same major version and matching metric-definition hash |
| 11 | Result store | Content-addressed files plus SQLite index; no database server |
| 12 | Web UI order | Staging and results first, then upload, setup, compare; canvas landmark overlay; inline SVG plots; localhost only, token before any network exposure |
| 13 | API | Upload and list videos, list tests, start run, fetch result by key, export CSV |
| 14 | GCP | Hard budget about $100 total with alerts; one bucket (`datasets/`, `runs/`, `models/`); official PyTorch GPU container; verify quota, region and spot pricing at signup |
| 15 | Validation | Technical only: synthetic metric recovery, repeat-measure reliability, agreement with own frame-by-frame annotation of about 20 clips; describe a clinical study, do not run one; release blocked until CI checks, these tests and model cards pass |
| 16 | Privacy | Hand-only framing, hashed filenames, metadata stripped at upload; append-only run log; immutable algorithm versions; one-page consent form for anyone filmed |
| 17 | Demo and summary | 3-minute demo (upload, gate, test, overlay, NPU benchmark, add a plug-in live); one-page summary |

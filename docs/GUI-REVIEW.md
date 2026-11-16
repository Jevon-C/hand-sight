# GUI review

Review of the web app from screenshots at phone (390 px), tablet (820 px) and desktop (1280 px)
widths, made with `scripts\screenshots.ps1`, plus the axe accessibility checker. The direction
is clinical, calm and light. The aim is a tool that looks and behaves like professional
software.

## Baseline

The automated accessibility scan (axe, WCAG 2.1 A and AA plus best practice) found no
violations on any of the ten desktop screens. The browser console shows only Trunk's harmless
`integrity` preload notice and one expected 404 (the screenshot of a result that does not exist).
Colour contrast, labels, headings and landmarks are sound. What is left is design quality, not
compliance.

## Findings, in order of effect

1. **Results are flat.** Every section is the same white card. The quality verdict, which decides
   whether to trust the numbers, sits beside the measures instead of leading the page. The
   plots are about 320 px tall each and tell little: one bar chart shows ten identical bars with
   no values; the y axis has two labels.
2. **Charts are not connected to the video.** Nothing links a point on the plot to a moment in
   the video, and there is no playhead. Hover and keyboard reading do not exist.
3. **Charts on a phone are unreadable.** The picture shrinks to half size, so axis text is about
   5 px.
4. **Staging is crowded.** Selecting a video opens a full-width black player that pushes the
   Analyse button below the fold. Every card carries two permanent "Not checked yet" rows for
   detectors that are not built. The title line started with a dangling "·".
5. **The flow is spread over three screens.** The chosen test is invisible on the staging
   screen, and the run panel sends the person away to choose one.
6. **Failures are dead ends.** "No such result." was a bare line of red text with no way forward.
7. **Compare shows differences as text only.** The column headings say "First result" and
   "Second result" instead of who, when and which hand. There is no chart of the two signals
   or of the landmark error.
8. **Phone header.** The navigation wrapped onto two rows and the status chips onto a third,
   using about a fifth of the screen.
9. **Terms are unexplained.** Palm lengths, arrests, hesitations and amplitude decrement have
   no definition on the screen where they appear.
10. **Fonts come from Google Fonts.** A tool for clinical data should not depend on a network
    call, and looks different offline.

## Done (polish pass 1)

- Focus ring survives Windows high-contrast mode; reduced-motion is respected.
- Phone header: brand and status on one row, menu below; the "Research prototype" chip is
  hidden at that width (the Illustrative banners remain).
- File chooser styled as a button; radio choices side by side; field spacing tightened.
- Video cards: one title line; the preview is smaller; the Analyse button sits above it.
- "No result is open" and failed results now explain themselves and offer where to go next.
- Chart text and lines are larger on a phone.

## Next

- Results page: a verdict summary first, measure cards with units and a one-line definition of
  each term, interactive charts linked to the video, a "show as table" option per chart.
- Compare: column headings with who, when and hand; overlaid signals; a landmark-error chart with
  the limits drawn on it.
- One guided flow: Video, Test, Analyse, with the chosen test always visible.
- Collapse the "not built yet" gate rows into one line.
- Self-host the font.
- Add the axe scan and a keyboard-only walk-through to `scripts\e2e.ps1`.

"""Browser end-to-end test: a person uses the app from upload to comparing two visits.

It starts the real server (development stand-in hand model, a temporary data folder) and drives
the built web app in a real browser with Playwright. Run it with ``scripts\\e2e.ps1``, which
builds everything first. The stand-in model reads the coloured dots of synthetic test videos,
so the test needs no recording and no model files.

The browser defaults to Microsoft Edge, which is on every Windows computer and plays the H.264
test videos; the browser Playwright installs for itself does not. Set ``AXONHAND_E2E_BROWSER``
to another Playwright channel, or to ``chromium`` to use Playwright's own build.
"""

from __future__ import annotations

import os
import re
import socket
import subprocess
import sys
import time
import urllib.error
import urllib.request
from collections.abc import Iterator
from pathlib import Path

import pytest
from playwright.sync_api import Browser, Page, expect, sync_playwright

ROOT = Path(__file__).resolve().parent.parent
SERVER = (
    ROOT
    / "target"
    / "debug"
    / ("axonhand-server.exe" if os.name == "nt" else "axonhand-server")
)
DIST = ROOT / "crates" / "ui" / "dist"
SLOW = 120_000  # milliseconds a job may take: the stand-in tracker reads every frame


def _free_port() -> int:
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return int(sock.getsockname()[1])


def _wait_until_up(url: str, process: subprocess.Popen[bytes]) -> None:
    deadline = time.monotonic() + 60
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise RuntimeError(
                "the server stopped while starting; run it by hand to see why"
            )
        try:
            with urllib.request.urlopen(f"{url}/api/health", timeout=2):
                return
        except (urllib.error.URLError, OSError):
            time.sleep(0.3)
    raise RuntimeError("the server did not start within a minute")


@pytest.fixture(scope="session")
def server(tmp_path_factory: pytest.TempPathFactory) -> Iterator[str]:
    if not SERVER.exists():
        pytest.fail(f"{SERVER} is missing; run scripts\\e2e.ps1, which builds it")
    if not (DIST / "index.html").exists():
        pytest.fail(
            f"{DIST} is missing; run scripts\\e2e.ps1, which builds the web app"
        )
    port = _free_port()
    data = tmp_path_factory.mktemp("data")
    env = {
        **os.environ,
        "AXONHAND_BIND": f"127.0.0.1:{port}",
        "AXONHAND_DATA_DIR": str(data),
        "AXONHAND_STATIC_DIR": str(DIST),
        "AXONHAND_PLUGINS_DIR": str(ROOT / "plugins"),
        "AXONHAND_PYTHON": sys.executable,
        "AXONHAND_HAND_MODEL": "stand_in",
    }
    process = subprocess.Popen([str(SERVER)], cwd=ROOT, env=env)
    url = f"http://127.0.0.1:{port}"
    try:
        _wait_until_up(url, process)
        yield url
    finally:
        process.terminate()
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            process.kill()


@pytest.fixture(scope="session")
def videos(tmp_path_factory: pytest.TempPathFactory) -> tuple[Path, Path]:
    folder = tmp_path_factory.mktemp("videos")
    made: list[Path] = []
    for name, rate in (("first", "2.0"), ("second", "3.0")):
        path = folder / f"{name}.mp4"
        subprocess.run(
            [
                sys.executable,
                "-m",
                "axonhand.tools.make_test_video",
                str(path),
                "--taps",
                "10",
                "--rate",
                rate,
            ],
            check=True,
            capture_output=True,
        )
        made.append(path)
    return made[0], made[1]


@pytest.fixture(scope="session")
def browser() -> Iterator[Browser]:
    choice = os.environ.get(
        "AXONHAND_E2E_BROWSER", "msedge" if os.name == "nt" else "chromium"
    )
    with sync_playwright() as playwright:
        launched = (
            playwright.chromium.launch()
            if choice == "chromium"
            else playwright.chromium.launch(channel=choice)
        )
        yield launched
        launched.close()


@pytest.fixture
def page(browser: Browser, server: str) -> Iterator[Page]:
    context = browser.new_context(
        base_url=server, viewport={"width": 1280, "height": 900}
    )
    opened = context.new_page()
    opened.set_default_timeout(15_000)
    problems: list[str] = []
    opened.on("pageerror", lambda error: problems.append(str(error)))
    opened.on(
        "console",
        lambda message: (
            problems.append(message.text) if message.type == "error" else None
        ),
    )
    yield opened
    context.close()
    assert not problems, f"the browser reported errors: {problems}"


AXE = Path(__file__).resolve().parent / "vendor" / "axe.min.js"

# What a keyboard user must be able to reach on the results screen, in any order.
RESULT_STOPS = {
    "Skip to content",
    "Analysis",
    "Comparison",
    "Library",
    "Download as CSV",
    "Print this report",
    "Compare results",
    "Set-up panel",
    "Video with the tracked hand",
    "Show as table",
    "Previous frame",
    "Play",
    "Next frame",
    "Save score",
}


def _assert_accessible(page: Page, screen: str) -> None:
    """Run the axe checker (bundled, so no network) and fail on any WCAG A/AA problem."""
    # Let the set-up panel finish sliding in: a half-faded panel fails the contrast check.
    page.wait_for_timeout(500)
    page.evaluate(AXE.read_text(encoding="utf-8"))
    found = page.evaluate(
        """async () => {
            const run = await axe.run(document, {
                runOnly: ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'],
            });
            return run.violations.map(v => ({
                id: v.id, impact: v.impact, help: v.help,
                nodes: v.nodes.slice(0, 3).map(n => n.target.join(' ')),
            }));
        }"""
    )
    assert not found, f"accessibility problems on the {screen} screen: {found}"


def _tab_through(page: Page, limit: int = 100) -> list[tuple[str, bool]]:
    """Press Tab from the top of the page; return each stop's name and whether it shows focus."""
    page.evaluate(
        """() => {
            window.scrollTo(0, 0);
            document.body.setAttribute('tabindex', '-1');
            document.body.focus();
            document.body.removeAttribute('tabindex');
        }"""
    )
    stops: list[tuple[str, bool]] = []
    for _ in range(limit):
        page.keyboard.press("Tab")
        stop = page.evaluate(
            """() => {
                const el = document.activeElement;
                if (!el || el === document.body) return null;
                const style = getComputedStyle(el);
                const ring = style.boxShadow !== 'none'
                    || (style.outlineStyle !== 'none' && parseFloat(style.outlineWidth) > 0);
                const name = el.getAttribute('aria-label') || el.innerText || el.value
                    || el.id || el.tagName;
                return [name.trim().split('\\n')[0], ring];
            }"""
        )
        if stop is None:
            break
        stops.append((stop[0], bool(stop[1])))
    return stops


def _go(page: Page, name: str) -> None:
    """Use the main menu, as a person would. Reloading would forget the chosen test."""
    page.get_by_role("navigation", name="Main").get_by_role("link", name=name).click()


def _open_setup(page: Page) -> None:
    """Open the set-up panel with its button. Open, it covers the menu, so leave it alone then."""
    search = page.get_by_label("Search or choose a test")
    toggle = page.get_by_role("button", name="Set-up panel")
    if not toggle.is_visible():
        # Not on the Analysis page yet, or the app is still starting.
        try:
            expect(toggle).to_be_visible(timeout=3_000)
        except AssertionError:
            _go(page, "Analysis")
            expect(toggle).to_be_visible()
    if toggle.get_attribute("aria-pressed") != "true":
        toggle.click()
    expect(search).to_be_visible()


def _choose_finger_tapping(page: Page) -> None:
    _open_setup(page)
    page.get_by_label("Search or choose a test").fill("Finger tapping")
    expect(page.get_by_role("heading", name="How to film this test")).to_be_visible()
    _assert_accessible(page, "set-up panel")


def _upload_and_analyse(page: Page, video: Path, code: str, date: str) -> str:
    """Upload a video in the set-up panel, analyse it, and return the key of the result."""
    _open_setup(page)
    page.get_by_role("button", name="Upload new").click()
    page.set_input_files("#video-file", str(video))
    page.get_by_label("Participant code").fill(code)
    page.get_by_label("Visit date").fill(date)
    page.get_by_role("radio", name="Left").check()
    page.get_by_role("button", name="Upload video").click()
    # Once the video is stored the panel goes back to the library with the new video chosen.
    expect(page.get_by_role("button", name="Upload new")).to_have_attribute(
        "aria-pressed", "false", timeout=SLOW
    )
    page.get_by_role("button", name="Analyse this video").click()
    page.wait_for_url(re.compile(r"\?key="), timeout=SLOW)
    match = re.search(r"key=([0-9a-f]+)", page.url)
    assert match, page.url
    return match.group(1)


def _frame_number(page: Page) -> int:
    text = page.get_by_role("status").filter(has_text="Frame").inner_text()
    found = re.match(r"Frame (\d+) of", text)
    assert found, text
    return int(found.group(1))


def _hand_pixels_drawn(page: Page) -> int:
    return int(
        page.evaluate(
            """() => {
                const canvas = document.querySelector('canvas.player__overlay');
                if (!canvas || !canvas.width || !canvas.height) return 0;
                const data = canvas.getContext('2d')
                    .getImageData(0, 0, canvas.width, canvas.height).data;
                let drawn = 0;
                for (let i = 3; i < data.length; i += 4) if (data[i] > 0) drawn += 1;
                return drawn;
            }"""
        )
    )


def test_two_visits_are_uploaded_analysed_watched_and_compared(
    page: Page, videos: tuple[Path, Path]
) -> None:
    first_video, second_video = videos
    outside: list[str] = []
    page.on(
        "request",
        lambda request: (
            outside.append(request.url)
            if request.url.split(":", 1)[0] in ("http", "https")
            and request.url.split("/")[2].split(":")[0]
            not in ("127.0.0.1", "localhost")
            else None
        ),
    )
    page.goto("/")
    expect(page.get_by_role("heading", level=1, name="New analysis")).to_be_visible()
    _assert_accessible(page, "analysis, empty")

    # Choose the test, then upload and analyse the first visit.
    _choose_finger_tapping(page)
    first = _upload_and_analyse(page, first_video, "P01", "2026-03-01")

    # The result: numbers, the warning that the stand-in is not a measurement, and the CSV.
    expect(page.get_by_role("heading", level=1, name="P01")).to_be_visible()
    expect(page.get_by_text("Illustrative.").first).to_be_visible()
    measures = page.get_by_role("list", name="Measures")
    expect(
        measures.get_by_role("listitem").filter(has_text="Tap count")
    ).to_be_visible()
    expect(page.get_by_role("button", name="Print this report")).to_be_visible()
    expect(page.get_by_role("link", name="Download as CSV")).to_have_attribute(
        "href", f"/api/results/{first}/export.csv"
    )

    # The video with the tracked hand, and the keyboard.
    expect(page.get_by_text(re.compile(r"Frame 1 of"))).to_be_visible(timeout=30_000)
    page.wait_for_function(
        "() => document.querySelector('video.preview')?.readyState >= 1", timeout=30_000
    )
    assert _hand_pixels_drawn(page) > 0, "no hand was drawn over the video"
    page.locator("div.player").focus()
    start = _frame_number(page)
    page.keyboard.press("ArrowRight")
    expect(page.get_by_text(re.compile(rf"Frame {start + 1} of"))).to_be_visible()
    page.keyboard.press("Shift+ArrowRight")
    expect(page.get_by_text(re.compile(rf"Frame {start + 11} of"))).to_be_visible()
    page.keyboard.press("ArrowLeft")
    expect(page.get_by_text(re.compile(rf"Frame {start + 10} of"))).to_be_visible()
    toggle = page.get_by_label("Show the tracked hand")
    expect(toggle).to_be_checked()
    page.keyboard.press("o")
    expect(toggle).not_to_be_checked()
    page.keyboard.press("o")
    expect(toggle).to_be_checked()

    # The whole screen is reachable with the keyboard, and every stop shows where focus is.
    expect(page.get_by_role("button", name="Show as table")).to_be_visible()
    stops = _tab_through(page)
    reached = {name for name, _ in stops}
    assert RESULT_STOPS <= reached, f"not reachable by Tab: {RESULT_STOPS - reached}"
    hidden = [name for name, ring in stops if not ring]
    assert not hidden, f"no visible focus indicator on: {hidden}"
    _assert_accessible(page, "results")

    # A clinician records their own score; a missing field is explained, not silently ignored.
    page.get_by_role("button", name="Save score").click()
    expect(page.get_by_text("Choose a score.")).to_be_visible()
    expect(page.get_by_text("Enter who is giving the score.")).to_be_visible()
    page.get_by_label("Your name or initials").fill("AB")
    page.get_by_label("Score", exact=True).select_option("3")
    page.get_by_label("Note (optional)").fill("slowing by the end")
    page.get_by_role("button", name="Save score").click()
    expect(page.get_by_text("Score saved.")).to_be_visible()
    given = page.get_by_role("table", name="Scores given")
    expect(given.get_by_role("row", name=re.compile("AB"))).to_contain_text(
        "3 · Moderate"
    )
    page.reload()
    expect(page.get_by_role("table", name="Scores given")).to_contain_text(
        "slowing by the end"
    )

    # The second visit, analysed the same way. The reload forgot the chosen test, so choose again.
    _choose_finger_tapping(page)
    second = _upload_and_analyse(page, second_video, "P01", "2026-06-01")
    assert second != first
    page.get_by_label("Your name or initials").fill("AB")
    page.get_by_label("Score", exact=True).select_option("1")
    page.get_by_role("button", name="Save score").click()
    expect(page.get_by_text("Score saved.")).to_be_visible()

    # Compare: start from the second result and pick the first as its partner.
    page.get_by_role("link", name="Compare results").click()
    page.wait_for_url(re.compile(r"/compare\?a="))
    expect(
        page.get_by_role("main").get_by_role("button", name="Compare")
    ).to_be_disabled()
    page.locator("#pick-b").select_option(index=1)
    page.get_by_role("main").get_by_role("link", name="Compare", exact=True).click()
    page.wait_for_url(re.compile(r"/compare\?a=.*&b="))
    measures = page.get_by_role("table", name="Measures compared")
    expect(measures.get_by_role("columnheader", name="Change")).to_be_visible()
    row = measures.get_by_role("row").filter(
        has=page.get_by_role("rowheader", name="Tap rate")
    )
    expect(row).to_have_count(1)
    change = row.get_by_role("cell").nth(2)
    # The arrow shows direction only; the number follows it.
    expect(change).to_have_text(re.compile(r"^\S*\s*[+\-\u2212]\d"))
    # The scores of the same rater are set side by side: this visit 1, the earlier visit 3.
    scores = page.get_by_role("table", name="Clinician scores compared")
    row_ab = scores.get_by_role("row", name=re.compile("AB"))
    expect(row_ab).to_contain_text("1 · Slight")
    expect(row_ab).to_contain_text("3 · Moderate")
    expect(row_ab.get_by_role("cell").nth(2)).to_have_text(re.compile(r"\+2$"))
    # Both results are stand-in results, so the page must say not to interpret them.
    expect(
        page.get_by_text("Do not interpret the differences.", exact=False)
    ).to_be_visible()

    _assert_accessible(page, "compare")

    # A result set against itself is allowed, but the page says so rather than hiding it.
    page.goto(f"/compare?a={first}&b={first}")
    expect(page.get_by_text("Both sides are the same result.")).to_be_visible()

    # Nothing on any screen contacted a site other than this app (fonts included).
    assert not outside, f"requests left the local app: {outside}"

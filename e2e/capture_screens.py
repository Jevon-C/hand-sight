"""Capture every screen of the web app at three widths, for design review.

It starts the real server the same way the browser test does (stand-in hand model, a temporary
data folder), makes two synthetic visits, and saves full-page screenshots of each screen at
phone, tablet and desktop widths. It also runs the axe accessibility checker on each screen when
it can download it, and writes what it finds to ``accessibility.txt`` next to the pictures.

Run it with ``scripts\\screenshots.ps1``, which builds everything first. The pictures go to
``target\\screenshots``. This is a review tool, not a test: it never fails because of how a
screen looks.
"""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
from collections.abc import Callable
from pathlib import Path

from playwright.sync_api import (
    Browser,
    BrowserContext,
    BrowserType,
    Page,
    expect,
    sync_playwright,
)

# The browser test owns the server start-up and the page-driving helpers; use them as they are.
from test_browser import (
    DIST,
    ROOT,
    SERVER,
    _choose_finger_tapping,
    _free_port,
    _go,
    _upload_and_analyse,
    _wait_until_up,
)

OUT = ROOT / "target" / "screenshots"
WIDTHS = {"phone": (390, 844), "tablet": (820, 1180), "desktop": (1280, 900)}
NO_SUCH_RESULT = "0" * 64


def _make_video(folder: Path, name: str, rate: str) -> Path:
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
    return path


def _launch(chromium: BrowserType) -> Browser:
    choice = os.environ.get(
        "AXONHAND_E2E_BROWSER", "msedge" if os.name == "nt" else "chromium"
    )
    return (
        chromium.launch() if choice == "chromium" else chromium.launch(channel=choice)
    )


class Reviewer:
    """Takes the pictures and keeps the notes."""

    def __init__(self, browser: Browser, base_url: str) -> None:
        self.browser = browser
        self.base_url = base_url
        self.axe_notes: list[str] = []
        self.console_notes: list[str] = []
        self.axe_source: str | None = None
        self.axe_tried = False

    def context(self, width: int, height: int) -> BrowserContext:
        return self.browser.new_context(
            base_url=self.base_url,
            viewport={"width": width, "height": height},
            device_scale_factor=1,
        )

    def page(self, context: BrowserContext, label: str) -> Page:
        opened = context.new_page()
        opened.set_default_timeout(20_000)
        opened.on(
            "console",
            lambda message: (
                self.console_notes.append(f"{label}: {message.text}")
                if message.type in ("error", "warning")
                else None
            ),
        )
        opened.on(
            "pageerror", lambda error: self.console_notes.append(f"{label}: {error}")
        )
        return opened

    def shoot(self, page: Page, screen: str, size: str) -> None:
        OUT.mkdir(parents=True, exist_ok=True)
        page.wait_for_load_state("networkidle")
        page.wait_for_timeout(400)
        page.screenshot(path=str(OUT / f"{size}-{screen}.png"), full_page=True)
        print(f"  {size}-{screen}.png")
        if size == "desktop":
            self.check_accessibility(page, screen)

    def check_accessibility(self, page: Page, screen: str) -> None:
        """Run axe on the screen as it is. Skipped quietly when it cannot be downloaded."""
        if not self.axe_tried:
            self.axe_tried = True
            bundled = Path(__file__).resolve().parent / "vendor" / "axe.min.js"
            if bundled.exists():
                self.axe_source = bundled.read_text(encoding="utf-8")
        if not self.axe_source:
            return
        page.evaluate(self.axe_source)
        found = page.evaluate(
            """async () => {
                const run = await axe.run(document, {
                    runOnly: ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'best-practice'],
                });
                return run.violations.map(v => ({
                    id: v.id, impact: v.impact, help: v.help,
                    nodes: v.nodes.slice(0, 3).map(n => n.target.join(' ')),
                    count: v.nodes.length,
                }));
            }"""
        )
        if not found:
            self.axe_notes.append(f"{screen}: no violations")
            return
        for item in found:
            self.axe_notes.append(
                f"{screen}: [{item['impact']}] {item['id']} - {item['help']} "
                f"({item['count']} places, e.g. {json.dumps(item['nodes'])})"
            )


def _each_width(
    review: Reviewer, screens: Callable[[Reviewer, Page, str], None]
) -> None:
    for size, (width, height) in WIDTHS.items():
        print(size)
        context = review.context(width, height)
        try:
            screens(review, review.page(context, size), size)
        finally:
            context.close()


def _empty_screens(review: Reviewer, page: Page, size: str) -> None:
    page.goto("/")
    expect(page.get_by_role("heading", level=1, name="New analysis")).to_be_visible()
    review.shoot(page, "analysis-empty", size)
    if size == "desktop":
        page.keyboard.press("Tab")
        review.shoot(page, "keyboard-focus-skip-link", size)
    _choose_finger_tapping(page)
    review.shoot(page, "analysis-test-chosen", size)
    page.goto(f"/?key={NO_SUCH_RESULT}")
    expect(page.get_by_role("alert")).to_be_visible()
    review.shoot(page, "analysis-error", size)
    page.goto("/compare")
    review.shoot(page, "compare-empty", size)
    _go(page, "Library")
    review.shoot(page, "library-videos-empty", size)


def _seed(review: Reviewer, videos: tuple[Path, Path]) -> tuple[str, str]:
    """Two visits of one participant, each with a clinician score, made as a person would."""
    context = review.context(*WIDTHS["desktop"])
    try:
        page = review.page(context, "seed")
        page.goto("/")
        keys: list[str] = []
        for video, date, score in (
            (videos[0], "2026-03-01", "3"),
            (videos[1], "2026-06-01", "1"),
        ):
            _choose_finger_tapping(page)
            keys.append(_upload_and_analyse(page, video, "P01", date))
            page.get_by_label("Your name or initials").fill("AB")
            page.get_by_label("Score", exact=True).select_option(score)
            page.get_by_label("Note (optional)").fill("reviewed for the screenshots")
            page.get_by_role("button", name="Save score").click()
            expect(page.get_by_text("Score saved.")).to_be_visible()
        return keys[0], keys[1]
    finally:
        context.close()


def _screens_with_data(
    first: str, second: str
) -> Callable[[Reviewer, Page, str], None]:
    def screens(review: Reviewer, page: Page, size: str) -> None:
        # A new page starts blank; the app must load before the menu exists.
        page.goto("/")
        _choose_finger_tapping(page)
        page.get_by_role("button", name=re.compile("P01")).first.click()
        expect(page.get_by_role("button", name="Analyse this video")).to_be_enabled()
        review.shoot(page, "analysis-ready", size)

        page.goto(f"/?key={second}")
        expect(page.get_by_text(re.compile(r"Frame 1 of"))).to_be_visible(
            timeout=30_000
        )
        review.shoot(page, "analysis-done", size)
        page.get_by_role("button", name="Set-up panel").click()
        review.shoot(page, "analysis-panel-over-result", size)

        page.get_by_role("button", name="Close").click()
        _go(page, "Library")
        review.shoot(page, "library-videos", size)
        page.get_by_role("tab", name="Tests").click()
        review.shoot(page, "library-tests", size)
        page.get_by_role("tab", name="Results").click()
        expect(page.get_by_role("table", name="Saved results")).to_be_visible()
        review.shoot(page, "library-results", size)

        page.goto(f"/compare?a={second}")
        expect(page.locator("#pick-b")).to_be_visible()
        review.shoot(page, "compare-pick", size)

        page.goto(f"/compare?a={second}&b={first}")
        expect(page.get_by_role("table", name="Measures compared")).to_be_visible()
        review.shoot(page, "compare", size)

    return screens


def main() -> int:
    if not SERVER.exists() or not (DIST / "index.html").exists():
        print("Build first: run scripts\\screenshots.ps1, which builds everything.")
        return 1
    port = _free_port()
    data = ROOT / "target" / "screenshots-data"
    data.mkdir(parents=True, exist_ok=True)
    folder = ROOT / "target" / "screenshots-videos"
    folder.mkdir(parents=True, exist_ok=True)
    for old in OUT.glob("*") if OUT.exists() else []:
        old.unlink()
    env = {
        **os.environ,
        "AXONHAND_BIND": f"127.0.0.1:{port}",
        "AXONHAND_DATA_DIR": str(data),
        "AXONHAND_STATIC_DIR": str(DIST),
        "AXONHAND_PLUGINS_DIR": str(ROOT / "plugins"),
        "AXONHAND_PYTHON": sys.executable,
        "AXONHAND_HAND_MODEL": "stand_in",
    }
    url = f"http://127.0.0.1:{port}"
    process = subprocess.Popen([str(SERVER)], cwd=ROOT, env=env)
    try:
        _wait_until_up(url, process)
        videos = (
            _make_video(folder, "first", "2.0"),
            _make_video(folder, "second", "3.0"),
        )
        with sync_playwright() as playwright:
            browser = _launch(playwright.chromium)
            try:
                review = Reviewer(browser, url)
                _each_width(review, _empty_screens)
                first, second = _seed(review, videos)
                _each_width(review, _screens_with_data(first, second))
            finally:
                browser.close()
        notes = ["Accessibility scan (axe, desktop width)"]
        notes += review.axe_notes or ["not run"]
        notes += ["", "Browser console messages"]
        notes += review.console_notes or ["none"]
        OUT.mkdir(parents=True, exist_ok=True)
        (OUT / "accessibility.txt").write_text(
            "\n".join(notes) + "\n", encoding="utf-8"
        )
        print(f"\nPictures and notes are in {OUT}")
        return 0
    finally:
        process.terminate()
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            process.kill()


if __name__ == "__main__":
    sys.exit(main())

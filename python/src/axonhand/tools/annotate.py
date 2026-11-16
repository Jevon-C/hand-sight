"""A small local page for marking taps on a video.

Usage::

    python -m axonhand.tools.annotate VIDEO [--port 8765]

Opens a page on this computer only (127.0.0.1) where you play the video, step frame by frame and
press ``M`` at each tap. ``S`` saves ``<video>.taps.json`` beside the video. Stop with Ctrl+C.
"""

from __future__ import annotations

import argparse
import json
import sys
import webbrowser
from functools import partial
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path
from typing import Any

from axonhand.evaluation import tapmarks
from axonhand.evaluation.tapmarks import AnnotationError, TapAnnotation
from axonhand.tools.annotate_page import PAGE
from axonhand.tools.probe import ProbeError, probe

MAX_BODY = 1_000_000


class _Handler(BaseHTTPRequestHandler):
    """Serves the page, the one video it was started with, and saves its marks."""

    def __init__(self, video: Path, fps: float, *args: Any, **kwargs: Any) -> None:
        self.video = video
        self.fps = fps
        super().__init__(*args, **kwargs)

    def log_message(self, format: str, *args: Any) -> None:  # noqa: A002
        """Keep the terminal quiet."""

    def _send(self, status: int, body: bytes, content_type: str) -> None:
        self.send_response(status)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def _json(self, status: int, data: dict[str, Any]) -> None:
        self._send(status, json.dumps(data).encode(), "application/json")

    def do_GET(self) -> None:  # noqa: N802 - name fixed by the base class
        """Serve the page, the video details or the video bytes."""
        if self.path == "/":
            self._send(200, PAGE.encode(), "text/html; charset=utf-8")
        elif self.path == "/info":
            try:
                existing = tapmarks.load(self.video)
            except AnnotationError:
                existing = None
            self._json(
                200,
                {
                    "video": self.video.name,
                    "fps": self.fps,
                    "tap_times": list(existing.times) if existing else [],
                    "annotator": existing.annotator if existing else "",
                    "note": existing.note if existing else "",
                },
            )
        elif self.path == "/video":
            self._serve_video()
        else:
            self._json(404, {"error": "not found"})

    def _serve_video(self) -> None:
        """Send the video, honouring a single ``Range`` request so the browser can seek."""
        size = self.video.stat().st_size
        start, end = 0, size - 1
        header = self.headers.get("Range", "")
        status = 200
        if header.startswith("bytes="):
            first, _, last = header[6:].partition("-")
            try:
                start = int(first) if first else max(0, size - int(last))
                end = int(last) if first and last else size - 1
            except ValueError:
                start, end = 0, size - 1
            end = min(end, size - 1)
            if start > end:
                self.send_response(416)
                self.send_header("Content-Range", f"bytes */{size}")
                self.send_header("Content-Length", "0")
                self.end_headers()
                return
            status = 206
        length = end - start + 1
        self.send_response(status)
        self.send_header("Content-Type", "video/mp4")
        self.send_header("Accept-Ranges", "bytes")
        self.send_header("Content-Length", str(length))
        if status == 206:
            self.send_header("Content-Range", f"bytes {start}-{end}/{size}")
        self.end_headers()
        with self.video.open("rb") as handle:
            handle.seek(start)
            remaining = length
            while remaining > 0:
                chunk = handle.read(min(65536, remaining))
                if not chunk:
                    break
                try:
                    self.wfile.write(chunk)
                except (BrokenPipeError, ConnectionResetError):
                    return  # the browser closed the connection (it seeked elsewhere)
                remaining -= len(chunk)

    def do_POST(self) -> None:  # noqa: N802 - name fixed by the base class
        """Save the marks."""
        if self.path != "/save":
            self._json(404, {"error": "not found"})
            return
        try:
            length = int(self.headers.get("Content-Length", "0"))
            if not 0 < length <= MAX_BODY:
                raise ValueError("bad request size")
            data = json.loads(self.rfile.read(length))
            annotation = TapAnnotation(
                video=self.video.name,
                times=tuple(float(t) for t in data["tap_times"]),
                annotator=str(data.get("annotator", ""))[:100],
                note=str(data.get("note", ""))[:1000],
            )
            path = tapmarks.save(self.video, annotation)
        except (ValueError, KeyError, TypeError, AnnotationError) as error:
            self._json(400, {"error": str(error)})
            return
        self._json(200, {"path": str(path)})


def make_server(video: Path, port: int = 0) -> HTTPServer:
    """A server bound to this computer only, for ``video``. ``port`` 0 picks a free one.

    Raises:
        ProbeError: if ``video`` cannot be read.
    """
    fps = probe(video).fps
    return HTTPServer(("127.0.0.1", port), partial(_Handler, video, fps))


def main(argv: list[str] | None = None) -> int:
    """Command line entry point. Returns a process exit code."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("video", type=Path)
    parser.add_argument("--port", type=int, default=8765)
    parser.add_argument("--no-browser", action="store_true")
    args = parser.parse_args(argv)
    video = args.video.resolve()
    try:
        server = make_server(video, args.port)
    except (ProbeError, OSError) as error:
        print(f"cannot start: {error}", file=sys.stderr)
        return 1
    url = f"http://127.0.0.1:{server.server_address[1]}/"
    print(f"Marking taps on {video.name}. Open {url} (Ctrl+C to stop).")
    if not args.no_browser:
        webbrowser.open(url)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        print("stopped")
    return 0


if __name__ == "__main__":
    sys.exit(main())

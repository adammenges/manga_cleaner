#!/usr/bin/env python3
"""
Browser UI for process_manga.py.

Features:
- Select a series folder (manual path or native macOS folder picker)
- Show cover inside the app
- Show the processing plan inside a live terminal-style panel
- Execute processing and watch output stream live
"""

from __future__ import annotations

import argparse
import json
import mimetypes
import shlex
import subprocess
import sys
import threading
import time
import urllib.parse
import webbrowser
from dataclasses import dataclass, field
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any, Optional

SCRIPT_PATH = Path(__file__).resolve().with_name("process_manga.py")
IMAGE_EXTS = {".jpg", ".jpeg", ".png", ".webp", ".bmp", ".gif"}
MAX_LOG_LINES = 6000


def pick_default_worker_python() -> str:
    candidate = Path(__file__).resolve().parent / ".venv" / "bin" / "python"
    if candidate.is_file():
        return str(candidate)
    return sys.executable


HTML_PAGE = """<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Manga Cleaner Console</title>
  <style>
    :root {
      --bg-1: #0d1b2a;
      --bg-2: #1b263b;
      --panel: #f8fafc;
      --line: #d7deea;
      --ink: #162232;
      --muted: #5e6d7f;
      --accent: #0b7285;
      --accent-strong: #075985;
      --danger: #b02a37;
      --terminal-bg: #0a0f15;
      --terminal-fg: #cae6b8;
      --terminal-muted: #8db08f;
    }

    * { box-sizing: border-box; }

    body {
      margin: 0;
      min-height: 100vh;
      background:
        radial-gradient(circle at 10% 20%, rgba(44, 120, 115, 0.25), transparent 38%),
        radial-gradient(circle at 85% 12%, rgba(17, 77, 146, 0.30), transparent 42%),
        linear-gradient(145deg, var(--bg-1), var(--bg-2));
      color: var(--ink);
      font-family: "Avenir Next", "SF Pro Display", "Helvetica Neue", Helvetica, Arial, sans-serif;
      display: flex;
      justify-content: center;
      padding: 26px 16px;
    }

    .app {
      width: min(1240px, 100%);
      background: rgba(255, 255, 255, 0.94);
      backdrop-filter: blur(5px);
      border: 1px solid rgba(255, 255, 255, 0.35);
      border-radius: 18px;
      box-shadow: 0 18px 45px rgba(4, 21, 36, 0.32);
      padding: 20px;
      display: grid;
      gap: 16px;
    }

    .topbar {
      display: flex;
      justify-content: space-between;
      gap: 12px;
      align-items: center;
    }

    .title {
      margin: 0;
      font-size: clamp(1.32rem, 2.2vw, 1.7rem);
      font-weight: 700;
      letter-spacing: 0.01em;
      color: #13273d;
    }

    .status {
      border-radius: 999px;
      padding: 8px 14px;
      font-size: 0.84rem;
      font-weight: 700;
      background: #dbeafe;
      color: #173b66;
      border: 1px solid #b6d4ff;
      white-space: nowrap;
    }

    .status.running {
      background: #fff4d6;
      border-color: #ffd26e;
      color: #6e4b0a;
      animation: pulse 1.5s ease-in-out infinite;
    }

    .status.error {
      background: #ffe0e3;
      border-color: #ffb6bf;
      color: #7c1120;
    }

    @keyframes pulse {
      0% { box-shadow: 0 0 0 0 rgba(255, 181, 60, 0.38); }
      75% { box-shadow: 0 0 0 9px rgba(255, 181, 60, 0); }
      100% { box-shadow: 0 0 0 0 rgba(255, 181, 60, 0); }
    }

    .card {
      background: var(--panel);
      border: 1px solid var(--line);
      border-radius: 14px;
      padding: 14px;
    }

    .card h2 {
      margin: 0 0 10px;
      font-size: 0.95rem;
      letter-spacing: 0.02em;
      text-transform: uppercase;
      color: #33465e;
    }

    .folder-row {
      display: grid;
      grid-template-columns: 1fr auto auto;
      gap: 8px;
    }

    input[type="text"] {
      width: 100%;
      padding: 11px 12px;
      border-radius: 10px;
      border: 1px solid #cad5e3;
      background: #fff;
      color: #1c2f45;
      font-size: 0.95rem;
    }

    input[type="text"]:focus {
      outline: 2px solid rgba(11, 114, 133, 0.26);
      border-color: var(--accent);
    }

    .button-row {
      margin-top: 10px;
      display: flex;
      flex-wrap: wrap;
      gap: 8px;
    }

    button {
      border: none;
      border-radius: 10px;
      padding: 10px 14px;
      font-size: 0.9rem;
      font-weight: 700;
      cursor: pointer;
      transition: transform 120ms ease, filter 120ms ease, opacity 120ms ease;
      color: #fff;
      background: linear-gradient(160deg, var(--accent), var(--accent-strong));
    }

    button.secondary {
      background: linear-gradient(160deg, #597084, #3d5369);
    }

    button.danger {
      background: linear-gradient(160deg, #c44c4c, var(--danger));
    }

    button.ghost {
      background: transparent;
      border: 1px solid #bccad8;
      color: #30475e;
    }

    button:disabled {
      opacity: 0.5;
      cursor: not-allowed;
      transform: none;
      filter: grayscale(35%);
    }

    button:not(:disabled):hover {
      transform: translateY(-1px);
      filter: brightness(1.04);
    }

    .split {
      display: grid;
      grid-template-columns: 1.7fr 1fr;
      gap: 14px;
      min-height: 520px;
    }

    .terminal-head {
      display: flex;
      justify-content: space-between;
      align-items: baseline;
      margin-bottom: 8px;
      color: #2d4760;
      font-size: 0.86rem;
      font-weight: 700;
    }

    .terminal {
      margin: 0;
      width: 100%;
      height: calc(100% - 24px);
      min-height: 420px;
      border-radius: 10px;
      border: 1px solid #1b2939;
      background: linear-gradient(180deg, #0a1119, var(--terminal-bg));
      color: var(--terminal-fg);
      padding: 12px;
      overflow: auto;
      font-size: 13px;
      line-height: 1.5;
      white-space: pre-wrap;
      word-break: break-word;
      font-family: "SF Mono", "Menlo", "Monaco", "Consolas", monospace;
      box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.03);
    }

    .terminal .muted {
      color: var(--terminal-muted);
    }

    .preview {
      display: grid;
      grid-template-rows: auto 1fr auto;
      gap: 10px;
      min-height: 520px;
    }

    .cover-box {
      border: 1px dashed #b7c6d6;
      background: #eef3f8;
      border-radius: 10px;
      min-height: 340px;
      display: grid;
      place-items: center;
      overflow: hidden;
      color: #4f657c;
      text-align: center;
      padding: 12px;
    }

    .cover-box img {
      width: 100%;
      height: 100%;
      object-fit: contain;
      display: none;
      background: #fff;
      border-radius: 8px;
    }

    .cover-meta {
      font-size: 0.84rem;
      color: var(--muted);
      word-break: break-all;
    }

    .tips {
      margin: 0;
      padding: 0;
      list-style: none;
      color: #526881;
      font-size: 0.85rem;
      line-height: 1.5;
    }

    @media (max-width: 980px) {
      .split {
        grid-template-columns: 1fr;
        min-height: 0;
      }
      .terminal {
        min-height: 340px;
      }
      .preview {
        min-height: 0;
      }
      .folder-row {
        grid-template-columns: 1fr;
      }
    }
  </style>
</head>
<body>
  <main class="app">
    <section class="topbar">
      <h1 class="title">Manga Cleaner Console</h1>
      <div id="statusPill" class="status">Idle</div>
    </section>

    <section class="card">
      <h2>Series Folder</h2>
      <div class="folder-row">
        <input id="seriesPath" type="text" spellcheck="false" placeholder="/path/to/series-folder">
        <button id="setPathBtn" class="secondary">Set Folder</button>
        <button id="browseBtn">Browse macOS</button>
      </div>
      <div class="button-row">
        <button id="showCoverBtn">Show Cover</button>
        <button id="showPlanBtn" class="secondary">Show Plan</button>
        <button id="runBtn" class="danger">Commit + Process</button>
        <button id="clearBtn" class="ghost">Clear Terminal</button>
      </div>
    </section>

    <section class="split">
      <section class="card">
        <div class="terminal-head">
          <span>Live Output</span>
          <span id="terminalHint" class="muted">All actions stream here</span>
        </div>
        <pre id="terminal" class="terminal"></pre>
      </section>

      <section class="card preview">
        <h2>Cover Preview</h2>
        <div id="coverBox" class="cover-box">
          <span id="coverEmpty">Run <b>Show Cover</b> to load the resolved cover in-app.</span>
          <img id="coverImage" alt="Series cover preview">
        </div>
        <div id="coverPath" class="cover-meta">No cover loaded.</div>
        <ul class="tips">
          <li>Show Plan prints the full dry-run plan in the terminal panel.</li>
          <li>Commit + Process runs with <code>--yes</code> and streams progress live.</li>
        </ul>
      </section>
    </section>
  </main>

  <script>
    const terminal = document.getElementById("terminal");
    const statusPill = document.getElementById("statusPill");
    const terminalHint = document.getElementById("terminalHint");
    const seriesPath = document.getElementById("seriesPath");
    const coverEmpty = document.getElementById("coverEmpty");
    const coverImage = document.getElementById("coverImage");
    const coverPath = document.getElementById("coverPath");

    const setPathBtn = document.getElementById("setPathBtn");
    const browseBtn = document.getElementById("browseBtn");
    const showCoverBtn = document.getElementById("showCoverBtn");
    const showPlanBtn = document.getElementById("showPlanBtn");
    const runBtn = document.getElementById("runBtn");
    const clearBtn = document.getElementById("clearBtn");

    let stateVersion = 0;
    let pollTimer = null;
    let localEditActive = false;
    let knownCoverPath = "";

    function addTerminal(text) {
      const atBottom = terminal.scrollTop + terminal.clientHeight >= terminal.scrollHeight - 24;
      terminal.textContent += text;
      if (atBottom) {
        terminal.scrollTop = terminal.scrollHeight;
      }
    }

    async function postJSON(url, body) {
      const resp = await fetch(url, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body || {}),
      });
      const payload = await resp.json();
      if (!resp.ok || payload.ok === false) {
        throw new Error(payload.error || "Request failed");
      }
      return payload;
    }

    function setBusy(running, statusText) {
      showCoverBtn.disabled = running;
      showPlanBtn.disabled = running;
      runBtn.disabled = running;
      browseBtn.disabled = running;
      setPathBtn.disabled = running;

      statusPill.textContent = statusText;
      statusPill.classList.remove("running", "error");
      if (running) {
        statusPill.classList.add("running");
        terminalHint.textContent = "Action running...";
      } else if (statusText.toLowerCase().includes("failed") || statusText.toLowerCase().includes("error")) {
        statusPill.classList.add("error");
        terminalHint.textContent = "Last action failed";
      } else {
        terminalHint.textContent = "Ready";
      }
    }

    function updateCover(path) {
      if (!path) {
        coverImage.style.display = "none";
        coverImage.removeAttribute("src");
        coverEmpty.style.display = "block";
        coverPath.textContent = "No cover loaded.";
        knownCoverPath = "";
        return;
      }

      if (knownCoverPath !== path) {
        coverImage.src = `/api/cover?path=${encodeURIComponent(path)}&t=${Date.now()}`;
        knownCoverPath = path;
      }
      coverImage.style.display = "block";
      coverEmpty.style.display = "none";
      coverPath.textContent = path;
    }

    async function pollState() {
      try {
        const resp = await fetch(`/api/state?from=${stateVersion}`);
        const data = await resp.json();
        if (data.ok === false) {
          throw new Error(data.error || "State request failed");
        }

        if (!localEditActive && typeof data.series_dir === "string") {
          seriesPath.value = data.series_dir;
        }
        setBusy(Boolean(data.running), data.status || "Idle");

        if (Array.isArray(data.new_log) && data.new_log.length) {
          addTerminal(data.new_log.join("\\n") + "\\n");
        }
        stateVersion = data.version || stateVersion;

        updateCover(data.cover_path || "");
      } catch (err) {
        setBusy(false, `Error: ${err.message}`);
      } finally {
        pollTimer = setTimeout(pollState, 450);
      }
    }

    async function runAction(action) {
      if (action === "process") {
        const ok = window.confirm("This will move files and write covers. Continue?");
        if (!ok) {
          return;
        }
      }
      try {
        await postJSON("/api/run", { action });
      } catch (err) {
        window.alert(err.message);
      }
    }

    setPathBtn.addEventListener("click", async () => {
      try {
        await postJSON("/api/set-folder", { path: seriesPath.value });
      } catch (err) {
        window.alert(err.message);
      }
    });

    browseBtn.addEventListener("click", async () => {
      try {
        const result = await postJSON("/api/browse-folder", {});
        if (result.path) {
          seriesPath.value = result.path;
          await postJSON("/api/set-folder", { path: result.path });
        }
      } catch (err) {
        window.alert(err.message);
      }
    });

    showCoverBtn.addEventListener("click", () => runAction("show_cover"));
    showPlanBtn.addEventListener("click", () => runAction("preview"));
    runBtn.addEventListener("click", () => runAction("process"));

    clearBtn.addEventListener("click", async () => {
      try {
        terminal.textContent = "";
        await postJSON("/api/clear-log", {});
      } catch (err) {
        window.alert(err.message);
      }
    });

    seriesPath.addEventListener("focus", () => { localEditActive = true; });
    seriesPath.addEventListener("blur", () => { localEditActive = false; });
    seriesPath.addEventListener("keydown", async (event) => {
      if (event.key === "Enter") {
        event.preventDefault();
        setPathBtn.click();
      }
    });

    addTerminal("[UI] Ready. Select a series folder, then choose an action.\\n");
    pollState();
  </script>
</body>
</html>
"""


@dataclass
class SharedState:
    series_dir: str = ""
    running: bool = False
    status: str = "Idle"
    current_action: str = ""
    last_exit_code: Optional[int] = None
    cover_path: Optional[str] = None
    version: int = 0
    log_entries: list[tuple[int, str]] = field(default_factory=list)
    lock: threading.Lock = field(default_factory=threading.Lock)

    def append_log(self, line: str) -> None:
        text = line.rstrip("\n")
        with self.lock:
            self.version += 1
            self.log_entries.append((self.version, text))
            if len(self.log_entries) > MAX_LOG_LINES:
                overflow = len(self.log_entries) - MAX_LOG_LINES
                del self.log_entries[:overflow]

    def set_series_dir(self, value: str) -> None:
        with self.lock:
            self.series_dir = value
            self.version += 1

    def clear_log(self) -> None:
        with self.lock:
            self.log_entries.clear()
            self.version += 1

    def set_running(self, *, running: bool, status: str, action: str = "") -> None:
        with self.lock:
            self.running = running
            self.status = status
            self.current_action = action
            self.version += 1

    def finish_action(self, *, status: str, exit_code: int) -> None:
        with self.lock:
            self.running = False
            self.status = status
            self.current_action = ""
            self.last_exit_code = exit_code
            self.version += 1

    def update_cover(self, path: Optional[str]) -> None:
        with self.lock:
            self.cover_path = path
            self.version += 1

    def snapshot(self, from_version: int) -> dict[str, Any]:
        with self.lock:
            new_log = [line for v, line in self.log_entries if v > from_version]
            return {
                "ok": True,
                "series_dir": self.series_dir,
                "running": self.running,
                "status": self.status,
                "current_action": self.current_action,
                "last_exit_code": self.last_exit_code,
                "cover_path": self.cover_path,
                "version": self.version,
                "new_log": new_log,
            }


class UIController:
    def __init__(self, python_bin: str, initial_series_dir: str = "") -> None:
        self.python_bin = python_bin
        self.state = SharedState(series_dir=initial_series_dir)
        self.state.append_log(f"[UI] Worker Python: {self.python_bin}")
        if initial_series_dir:
            self.state.append_log(f"[UI] Initial folder: {initial_series_dir}")
        self._thread_lock = threading.Lock()
        self._worker: Optional[threading.Thread] = None

    def _validate_series_dir(self) -> Path:
        with self.state.lock:
            series_dir = self.state.series_dir.strip()
        if not series_dir:
            raise ValueError("Set a series folder first.")
        resolved = Path(series_dir).expanduser().resolve()
        if not resolved.is_dir():
            raise ValueError(f"Not a valid folder: {resolved}")
        return resolved

    def set_series_dir(self, path_str: str) -> str:
        cleaned = str(Path(path_str).expanduser()) if path_str else ""
        if not cleaned:
            raise ValueError("Folder path is empty.")
        resolved = Path(cleaned).resolve()
        if not resolved.is_dir():
            raise ValueError(f"Not a valid folder: {resolved}")
        self.state.set_series_dir(str(resolved))
        self.state.append_log(f"[UI] Series folder set: {resolved}")
        return str(resolved)

    def clear_log(self) -> None:
        self.state.clear_log()
        self.state.append_log("[UI] Terminal cleared.")

    def choose_folder_macos(self) -> Optional[str]:
        if sys.platform != "darwin":
            raise RuntimeError("Native folder picker is only available on macOS.")

        script_lines = [
            'set selectedFolder to choose folder with prompt "Select manga series folder"',
            "POSIX path of selectedFolder",
        ]
        cmd = ["/usr/bin/osascript"]
        for line in script_lines:
            cmd.extend(["-e", line])

        proc = subprocess.run(cmd, capture_output=True, text=True)
        if proc.returncode == 0:
            out = proc.stdout.strip()
            return out or None

        err = proc.stderr.strip()
        if "User canceled" in err or "(-128)" in err:
            return None
        raise RuntimeError(err or "macOS folder picker failed")

    def run_action(self, action: str) -> None:
        if action not in {"show_cover", "preview", "process"}:
            raise ValueError(f"Unknown action: {action}")
        if not SCRIPT_PATH.exists():
            raise RuntimeError(f"Missing script: {SCRIPT_PATH}")

        series_dir = self._validate_series_dir()

        with self._thread_lock:
            if self.state.running:
                raise RuntimeError("Another action is already running.")

            action_label = {
                "show_cover": "Show Cover",
                "preview": "Show Plan",
                "process": "Commit + Process",
            }[action]
            self.state.set_running(running=True, status=f"Running: {action_label}", action=action)

            worker = threading.Thread(
                target=self._run_action_worker,
                args=(action, series_dir),
                daemon=True,
            )
            self._worker = worker
            worker.start()

    def _run_action_worker(self, action: str, series_dir: Path) -> None:
        cmd = [self.python_bin, str(SCRIPT_PATH)]
        if action == "show_cover":
            cmd.append("--print-cover-path")
        elif action == "preview":
            cmd.append("--dry-run")
        else:
            cmd.append("--yes")
        cmd.append(str(series_dir))

        action_title = {
            "show_cover": "SHOW COVER",
            "preview": "SHOW PLAN",
            "process": "COMMIT + PROCESS",
        }[action]

        self.state.append_log("")
        self.state.append_log("=" * 92)
        self.state.append_log(f"[{action_title}] {time.strftime('%Y-%m-%d %H:%M:%S')}")
        self.state.append_log(f"$ {shlex.join(cmd)}")

        output_lines: list[str] = []
        rc = 1
        try:
            with subprocess.Popen(
                cmd,
                cwd=str(SCRIPT_PATH.parent),
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                text=True,
                bufsize=1,
            ) as proc:
                assert proc.stdout is not None
                for line in proc.stdout:
                    output_lines.append(line.rstrip("\n"))
                    self.state.append_log(line)
                rc = proc.wait()
        except Exception as exc:
            self.state.append_log(f"[UI-ERROR] Failed to run command: {exc}")
            rc = 1

        if action == "show_cover":
            resolved_cover = self._extract_cover_path(output_lines)
            if resolved_cover:
                self.state.update_cover(resolved_cover)
                self.state.append_log(f"[UI] Cover loaded in-app: {resolved_cover}")

        if rc == 0:
            self.state.append_log(f"[UI] Action completed successfully ({action}).")
            status = f"Done: {action.replace('_', ' ').title()}"
        else:
            self.state.append_log(f"[UI] Action failed ({action}) with exit code {rc}.")
            status = f"Failed ({rc})"

        self.state.finish_action(status=status, exit_code=rc)

    def _extract_cover_path(self, output_lines: list[str]) -> Optional[str]:
        for line in reversed(output_lines):
            candidate = line.strip()
            if not candidate:
                continue
            p = Path(candidate).expanduser()
            if p.is_file() and p.suffix.lower() in IMAGE_EXTS:
                return str(p.resolve())
        return None


class RequestHandler(BaseHTTPRequestHandler):
    controller: UIController

    def _send_json(self, payload: dict[str, Any], *, status: int = HTTPStatus.OK) -> None:
        data = json.dumps(payload).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Cache-Control", "no-store")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def _send_html(self, html: str) -> None:
        data = html.encode("utf-8")
        self.send_response(HTTPStatus.OK)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_header("Cache-Control", "no-store")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def _send_error_json(self, message: str, *, status: int = HTTPStatus.BAD_REQUEST) -> None:
        self._send_json({"ok": False, "error": message}, status=status)

    def _read_json_body(self) -> dict[str, Any]:
        try:
            length = int(self.headers.get("Content-Length", "0"))
        except ValueError:
            length = 0
        raw = self.rfile.read(length) if length > 0 else b"{}"
        try:
            parsed = json.loads(raw.decode("utf-8"))
        except json.JSONDecodeError as exc:
            raise ValueError(f"Invalid JSON body: {exc}") from exc
        if not isinstance(parsed, dict):
            raise ValueError("JSON body must be an object.")
        return parsed

    def do_GET(self) -> None:  # noqa: N802
        parsed = urllib.parse.urlparse(self.path)
        path = parsed.path

        if path == "/":
            return self._send_html(HTML_PAGE)

        if path == "/api/state":
            query = urllib.parse.parse_qs(parsed.query)
            try:
                from_version = int((query.get("from") or ["0"])[0])
            except ValueError:
                from_version = 0
            snapshot = self.controller.state.snapshot(from_version=from_version)
            return self._send_json(snapshot)

        if path == "/api/cover":
            query = urllib.parse.parse_qs(parsed.query)
            raw_path = (query.get("path") or [""])[0]
            if not raw_path:
                return self._send_error_json("Missing cover path.")

            cover_path = Path(raw_path).expanduser()
            if not cover_path.is_file():
                return self._send_error_json("Cover file does not exist.", status=HTTPStatus.NOT_FOUND)
            if cover_path.suffix.lower() not in IMAGE_EXTS:
                return self._send_error_json("Unsupported cover type.", status=HTTPStatus.BAD_REQUEST)

            try:
                data = cover_path.read_bytes()
            except OSError as exc:
                return self._send_error_json(
                    f"Failed to read cover file: {exc}",
                    status=HTTPStatus.INTERNAL_SERVER_ERROR,
                )
            content_type = mimetypes.guess_type(str(cover_path))[0] or "application/octet-stream"
            self.send_response(HTTPStatus.OK)
            self.send_header("Content-Type", content_type)
            self.send_header("Cache-Control", "no-store")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)
            return

        self.send_response(HTTPStatus.NOT_FOUND)
        self.end_headers()

    def do_POST(self) -> None:  # noqa: N802
        parsed = urllib.parse.urlparse(self.path)
        path = parsed.path

        try:
            body = self._read_json_body()
        except ValueError as exc:
            return self._send_error_json(str(exc))

        try:
            if path == "/api/set-folder":
                raw_path = str(body.get("path", "")).strip()
                resolved = self.controller.set_series_dir(raw_path)
                return self._send_json({"ok": True, "path": resolved})

            if path == "/api/browse-folder":
                picked = self.controller.choose_folder_macos()
                if not picked:
                    return self._send_json({"ok": True, "path": None})
                resolved = self.controller.set_series_dir(picked)
                return self._send_json({"ok": True, "path": resolved})

            if path == "/api/run":
                action = str(body.get("action", "")).strip()
                self.controller.run_action(action)
                return self._send_json({"ok": True, "started": action})

            if path == "/api/clear-log":
                self.controller.clear_log()
                return self._send_json({"ok": True})

            self.send_response(HTTPStatus.NOT_FOUND)
            self.end_headers()
            return
        except ValueError as exc:
            return self._send_error_json(str(exc))
        except RuntimeError as exc:
            return self._send_error_json(str(exc), status=HTTPStatus.CONFLICT)
        except Exception as exc:
            return self._send_error_json(f"Unexpected error: {exc}", status=HTTPStatus.INTERNAL_SERVER_ERROR)

    def log_message(self, format: str, *args: Any) -> None:
        return


def run_server(
    *,
    host: str,
    port: int,
    open_browser: bool,
    initial_series_dir: str,
    python_bin: str,
) -> int:
    controller = UIController(python_bin=python_bin, initial_series_dir=initial_series_dir)
    handler_cls = type("MangaUIHandler", (RequestHandler,), {"controller": controller})

    httpd = ThreadingHTTPServer((host, port), handler_cls)
    actual_host, actual_port = httpd.server_address[0], httpd.server_address[1]
    url = f"http://{actual_host}:{actual_port}/"

    print(f"[UI] Manga Cleaner Console listening at {url}")
    print("[UI] Press Ctrl+C to stop.")
    if open_browser:
        webbrowser.open(url)

    try:
        httpd.serve_forever()
    except KeyboardInterrupt:
        print("\n[UI] Shutting down...")
    finally:
        httpd.server_close()
    return 0


def main(argv: Optional[list[str]] = None) -> int:
    default_python_bin = pick_default_worker_python()
    parser = argparse.ArgumentParser(description="Launch browser UI for process_manga.py")
    parser.add_argument("series_dir", nargs="?", default="", help="Optional starting series folder path")
    parser.add_argument("--host", default="127.0.0.1", help="Bind host (default: 127.0.0.1)")
    parser.add_argument("--port", type=int, default=8765, help="Bind port (default: 8765)")
    parser.add_argument(
        "--no-open",
        action="store_true",
        help="Do not automatically open the browser.",
    )
    parser.add_argument(
        "--python-bin",
        default=default_python_bin,
        help="Python interpreter used to run process_manga.py (default: .venv/bin/python if present).",
    )
    args = parser.parse_args(argv)

    if args.series_dir:
        p = Path(args.series_dir).expanduser().resolve()
        if not p.is_dir():
            print(f"[ERROR] series_dir is not a directory: {p}", file=sys.stderr)
            return 2
        initial_series_dir = str(p)
    else:
        initial_series_dir = ""

    if not SCRIPT_PATH.exists():
        print(f"[ERROR] Missing script: {SCRIPT_PATH}", file=sys.stderr)
        return 2

    return run_server(
        host=args.host,
        port=args.port,
        open_browser=not args.no_open,
        initial_series_dir=initial_series_dir,
        python_bin=args.python_bin,
    )


if __name__ == "__main__":
    raise SystemExit(main())

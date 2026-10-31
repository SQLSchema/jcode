"""Live acceptance harness for the cracked fork (coordinator test tool).

Launches cracked.cmd in a fresh console with debug control enabled through
environment variables only (no config edits), drives it over the debug socket,
captures the real window and probes the sprite animation.

Usage:
    python live_check.py launch
    python live_check.py inject-chat
    python live_check.py shot <name>
    python live_check.py anim <x0,y0,x1,y1> [samples]
    python live_check.py debug <client:cmd>
    python live_check.py stop
"""

import hashlib
import os
import subprocess
import sys
import time
from pathlib import Path

from PIL import Image, ImageChops

sys.stdout.reconfigure(encoding="utf-8", errors="replace")

TMP = Path(os.environ["TEMP"])
LOCAL = Path(os.environ["LOCALAPPDATA"])
CRACKED = LOCAL / "jcode-cracked"
EXE = CRACKED / "bin" / "cracked.exe"
CMD = CRACKED / "bin" / "cracked.cmd"
SOCK = CRACKED / "run" / "cracked.sock"
CAP = TMP / "wincap.ps1"
OUT = Path(__file__).resolve().parent.parent / "previews"
TITLE = "cracked/"


def env():
    e = dict(os.environ)
    e["JCODE_HOME"] = str(CRACKED / "home")
    e["JCODE_RUNTIME_DIR"] = str(CRACKED / "run")
    e["JCODE_DEBUG_CONTROL"] = "1"
    e["JCODE_DEBUG_SOCKET"] = "1"
    return e


def debug(cmd: str) -> str:
    r = subprocess.run(
        [str(EXE), "debug", "--no-update", "--socket", str(SOCK), cmd],
        env=env(), capture_output=True, text=True, encoding="utf-8", errors="replace",
    )
    return (r.stdout + r.stderr).strip()


def capture(path: Path) -> bool:
    r = subprocess.run(
        ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(CAP),
         "-Match", TITLE, "-Out", str(path)],
        capture_output=True, text=True,
    )
    return path.exists() and "captured" in r.stdout


def stop():
    subprocess.run(["taskkill", "/im", "cracked.exe", "/f"], capture_output=True)
    for prefix in ("c", "d"):
        subprocess.run(
            ["taskkill", "/fi", f"WINDOWTITLE eq jcode:{prefix}:cracked*", "/f"],
            capture_output=True,
        )


def launch():
    stop()
    # Launch from the user's home folder: starting inside the jcode repo turns
    # on self-dev mode, which is not what the user runs day to day.
    home = os.environ.get("USERPROFILE", str(Path.home()))
    subprocess.Popen(
        ["cmd", "/c", "start", "", "/max", "/d", home, "cmd", "/k", str(CMD)],
        env=env(),
        cwd=home,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        creationflags=subprocess.DETACHED_PROCESS | subprocess.CREATE_NEW_PROCESS_GROUP,
        close_fds=True,
    )
    for _ in range(40):
        time.sleep(1)
        if "session_id" in debug("sessions"):
            time.sleep(3)
            maximize()
            print("ready")
            return
    print("launch timed out")


def maximize(width: int = 1920, height: int = 1040):
    """Size the cracked console like a maximized 1920x1080 window."""
    subprocess.run(
        ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(TMP / "winsize.ps1"),
         "-Width", str(width), "-Height", str(height)],
        capture_output=True,
    )
    time.sleep(1.5)


def _unused_maximize():
    """Old ShowWindow approach, kept for reference (ignored by conhost)."""
    ps = (
        "Add-Type @'\n"
        "using System; using System.Text; using System.Runtime.InteropServices;\n"
        "public class M { public delegate bool E(IntPtr h, IntPtr l);\n"
        "[DllImport(\"user32.dll\")] public static extern bool EnumWindows(E f, IntPtr l);\n"
        "[DllImport(\"user32.dll\")] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);\n"
        "[DllImport(\"user32.dll\")] public static extern bool ShowWindow(IntPtr h, int c); }\n"
        "'@\n"
        "[M]::EnumWindows({ param($h,$l) $s=New-Object Text.StringBuilder 256; "
        "[void][M]::GetWindowText($h,$s,256); if($s.ToString() -match 'cracked/'){ [void][M]::ShowWindow($h,3) }; $true }, [IntPtr]::Zero) | Out-Null"
    )
    subprocess.run(["powershell", "-NoProfile", "-Command", ps], capture_output=True)
    time.sleep(1.5)


def inject_chat():
    for role, text in [
        ("user", "hey rem, are you still there?"),
        ("assistant", "Still here! This reply is long enough to wrap across a couple of rows "
                      "so we can confirm the transcript stays inside the main column and never "
                      "runs underneath the sidebar on the right."),
        ("user", "show me the tokens card"),
    ]:
        print(debug(f"client:inject:{role}:{text}"))
    # A real keypress forces a full frame and counts as user interaction.
    debug("client:keys:a")
    debug("client:keys:backspace")
    time.sleep(1.5)


def shot(name: str):
    OUT.mkdir(parents=True, exist_ok=True)
    p = OUT / f"{name}.png"
    print(p if capture(p) else "capture failed")


def anim(box: str, samples: int = 10):
    region = tuple(int(v) for v in box.split(","))
    frames = []
    for i in range(samples):
        p = TMP / f"live_anim_{i}.png"
        if capture(p):
            frames.append(Image.open(p).convert("RGB"))
        time.sleep(0.15)
    hashes = [hashlib.md5(f.crop(region).tobytes()).hexdigest()[:6] for f in frames]
    print("sprite hashes:", hashes)
    print("distinct sprite frames:", len(set(hashes)))
    w, h = frames[0].size
    outside = [(0, 0, region[0], h), (region[2], 0, w, h), (region[0], region[3], region[2], h)]
    diffs = []
    for f in frames[1:]:
        for r in outside:
            if r[2] > r[0] and r[3] > r[1]:
                b = ImageChops.difference(frames[0].crop(r), f.crop(r)).getbbox()
                if b:
                    diffs.append((r, b))
    print("changes outside sprite:", diffs[:6])
    for p in TMP.glob("live_anim_*.png"):
        p.unlink()


if __name__ == "__main__":
    action = sys.argv[1]
    if action == "launch":
        launch()
    elif action == "inject-chat":
        inject_chat()
    elif action == "shot":
        shot(sys.argv[2])
    elif action == "anim":
        anim(sys.argv[2], int(sys.argv[3]) if len(sys.argv) > 3 else 10)
    elif action == "debug":
        print(debug(sys.argv[2]))
    elif action == "stop":
        stop()

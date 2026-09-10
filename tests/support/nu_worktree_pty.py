"""Verify actual Reedline Tab insertion, Enter execution, and browser history."""
import fcntl
import json
import os
import pathlib
import pty
import select
import signal
import struct
import subprocess
import sys
import termios
import time

root = pathlib.Path(sys.argv[1])
binary = pathlib.Path(sys.argv[2])
for name in ["GIT_DIR", "GIT_WORK_TREE", "GIT_COMMON_DIR"]:
    os.environ.pop(name, None)
repo = root / "repo with a ' quote 雪 [dir] $()"
nested = repo / "nested"
nested.mkdir(parents=True)
subprocess.run(["git", "init", "--quiet", str(repo)], check=True)
source = root / "init.nu"
settings = root / "config.toml"
settings.write_text("")
source.write_bytes(subprocess.check_output(
    [str(binary), "-C", str(settings), "init", "nu", "--no-setup-key-binding"], cwd=nested))
records = root / "records.jsonl"


def quote(value):
    hashes = "#"
    while "'" + hashes in str(value):
        hashes += "#"
    return "r" + hashes + "'" + str(value) + "'" + hashes


config = root / "terminal.nu"
config.write_text(f"""use {quote(source)} *
$env.config.show_banner = false
$env.config.edit_mode = 'emacs'
$env.config.shell_integration.osc133 = false
$env.PROMPT_COMMAND = {{|| 'CJPTY> '}}
$env.PROMPT_COMMAND_RIGHT = {{|| ''}}
def capture [] {{
    let row = {{buffer: (commandline), cwd: $env.PWD, history: $env.__cj_history}}
    (($row | to json -r) + (char nl)) | save --append {quote(records)}
}}
$env.config.keybindings ++= [{{name: capture, modifier: none, keycode: f2,
    mode: emacs, event: {{send: executehostcommand, cmd: 'capture'}}}}]
""")
pid, fd = pty.fork()
if pid == 0:
    os.chdir(nested)
    os.environ["TERM"] = "xterm-256color"
    os.environ["PATH"] = str(binary.parent) + os.pathsep + os.environ["PATH"]
    os.environ["XDG_CONFIG_HOME"] = str(root / "xdg")
    os.environ["HOME"] = str(root)
    os.execvp("nu", ["nu", "--no-history", "--config", str(config)])

fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 30, 150, 0, 0))
transcript = bytearray()


def wait_until(predicate):
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        if select.select([fd], [], [], 0.02)[0]:
            data = os.read(fd, 65536)
            transcript.extend(data)
            if b"\x1b[6n" in data:
                os.write(fd, b"\x1b[1;1R")
        if predicate():
            return
    raise AssertionError(transcript.decode(errors="replace"))


def rows():
    try:
        return [json.loads(line) for line in records.read_text().splitlines()]
    except (FileNotFoundError, json.JSONDecodeError):
        return []


def capture():
    count = len(rows())
    start = len(transcript)
    os.write(fd, b"\x1bOQ")
    wait_until(lambda: len(rows()) > count)
    wait_until(lambda: b"\x1b[?2004h" in transcript[start:])
    return rows()[-1]


def enter(text=""):
    start = len(transcript)
    os.write(fd, text.encode() + b"\r")
    wait_until(lambda: b"\x1b[?2004h" in transcript[start:])


try:
    wait_until(lambda: b"CJPTY> " in transcript)
    for flag in ["-jw", "--jump-worktree"]:
        for space in ["", " ", "  "]:
            os.write(fd, ("cd " + flag + space + "\t").encode())
            row = capture()
            assert row["cwd"] == str(nested) and row["history"] == [], row
            assert str(repo) in row["buffer"], row
            enter()
            row = capture()
            assert row["cwd"] == str(repo) and row["history"] == [str(nested)], row
            enter("cd v")
            row = capture()
            assert row["cwd"] == str(nested) and row["history"] == [], row
finally:
    (root / "terminal.log").write_bytes(transcript)
    os.kill(pid, signal.SIGKILL)
    os.close(fd)
    os.waitpid(pid, 0)

"""Exercise Nushell's actual Reedline keybinding in an isolated terminal."""
import fcntl
import json
import os
import pathlib
import pty
import select
import signal
import struct
import sys
import termios
import time

root = pathlib.Path(sys.argv[1])
actions = json.loads((root / "actions.json").read_text())
records = root / "records.jsonl"
records.unlink(missing_ok=True)
config = root / "terminal.nu"


def quote(value):
    hashes = "#"
    while "'" + hashes in str(value):
        hashes += "#"
    return "r" + hashes + "'" + str(value) + "'" + hashes


config.write_text(
    f"""use {quote(root / 'init.nu')} *
if '__cj_history' not-in ($env | columns) {{ error make {{msg: 'cj history was not initialized'}} }}
$env.config.show_banner = false
$env.config.shell_integration.osc133 = false
$env.config.edit_mode = 'emacs'
$env.PROMPT_COMMAND = {{|| 'CJPTY> '}}
$env.PROMPT_COMMAND_RIGHT = {{|| ''}}
$env.PROMPT_INDICATOR = ''
$env.__cj_history = {json.dumps(actions['history'], ensure_ascii=False)}
$env.CJ_INTERNAL_DOWN_ROUTE = '/sentinel/down/route'
def --env _cj-test-widget [] {{
    _cj-key-widget
    let row = {{buffer: (commandline), cursor: (commandline get-cursor),
        history: $env.__cj_history, cwd: $env.PWD, route: $env.CJ_INTERNAL_DOWN_ROUTE}}
    (($row | to json -r) + (char nl)) | save --append {quote(records)}
}}
$env.config.keybindings ++= [{{name: cj-test, modifier: control, keycode: char_o,
    mode: emacs, event: {{send: executehostcommand, cmd: '_cj-test-widget'}}}}]
"""
)

pid, fd = pty.fork()
if pid == 0:
    os.environ["TERM"] = "xterm-256color"
    os.environ["XDG_CONFIG_HOME"] = str(root / "xdg")
    os.environ["HOME"] = str(root)
    os.chdir(root)
    os.execvp("nu", ["nu", "--no-history", "--config", str(config)])

fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 30, 150, 0, 0))
transcript = bytearray()


def pump():
    if select.select([fd], [], [], 0.02)[0]:
        try:
            data = os.read(fd, 65536)
        except OSError:
            return
        transcript.extend(data)
        # Reedline requests cursor position while repainting after host commands.
        if b"\x1b[6n" in data:
            os.write(fd, b"\x1b[1;1R")


def wait_until(predicate):
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        pump()
        if predicate():
            return
    raise AssertionError(transcript.decode(errors="replace"))


def rows():
    if not records.exists():
        return []
    try:
        return [json.loads(line) for line in records.read_text().splitlines()]
    except (json.JSONDecodeError, UnicodeDecodeError):
        return []


try:
    wait_until(lambda: b"CJPTY> " in transcript)
    count = 0
    for action in actions["actions"]:
        before = len(transcript)
        os.write(fd, action["send"].encode())
        if action.get("record", False):
            count += 1
            wait_until(lambda: len(rows()) >= count)
            # Wait for the prompt hook before delivering the next key sequence.
            wait_until(lambda: b"\x1b[?2004h" in transcript[before:])
        else:
            # Ctrl+C and Enter create another prompt; wait for its redraw.
            wait_until(lambda: b"\x1b[?2004h" in transcript[before:])
    print(json.dumps(rows(), ensure_ascii=False), flush=True)
finally:
    (root / "terminal.log").write_bytes(transcript)
    try:
        os.kill(pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    os.close(fd)
    os.waitpid(pid, 0)

#!/usr/bin/env python3
"""Exercise real terminal input on an Android emulator; restores activity/settings."""
import argparse
import fcntl
import json
import os
import pty
import re
import select
import signal
import struct
import subprocess
import termios
import threading
import time
import xml.etree.ElementTree as ET
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--serial', required=True)
parser.add_argument('--binary', type=Path)
args = parser.parse_args()

def adb(*command):
    return subprocess.check_output(['adb', '-s', args.serial, *command], text=True, timeout=20)

assert adb('shell', 'getprop', 'ro.kernel.qemu').strip() == '1', 'Smoke test requires an emulator'
if args.binary is None:
    metadata = json.loads(subprocess.check_output(['cargo', 'metadata', '--no-deps', '--format-version', '1']))
    args.binary = Path(metadata['target_directory']) / 'debug' / 'ascii-scrcpy'

initial_activity = adb('shell', 'dumpsys', 'activity', 'activities')
match = re.search(r'(?:topResumedActivity|mResumedActivity)=.*? u\d+ ([\w.]+/[\w.]+)', initial_activity)
initial_component = match.group(1) if match else None
initial_forward = adb('forward', '--list')
rotation = {key: adb('shell', 'settings', 'get', 'system', key).strip() for key in ['accelerometer_rotation', 'user_rotation']}
remote_xml = '/data/local/tmp/ascii-scrcpy-smoke.xml'
process = None
master = slave = None
buffer = bytearray()
lock = threading.Lock()

def drain():
    while True:
        try:
            if not select.select([master], [], [], 0.2)[0]:
                if process.poll() is not None:
                    break
                continue
            data = os.read(master, 65536)
            if not data:
                break
            with lock:
                buffer.extend(data)
                if len(buffer) > 2_000_000:
                    del buffer[:1_000_000]
        except OSError:
            break

def output():
    with lock:
        return bytes(buffer).decode('utf-8', 'replace')

def wait_for(predicate, label, timeout=12):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if predicate():
            print(f'PASS {label}', flush=True)
            return
        if process.poll() is not None:
            raise AssertionError(f'App exited {process.returncode}: {output()[-2000:]}')
        time.sleep(0.1)
    raise AssertionError(f'Timed out: {label}; {output()[-600:]}')

def ui():
    adb('shell', 'uiautomator', 'dump', remote_xml)
    return ET.fromstring(adb('shell', 'cat', remote_xml))

def texts(root):
    return {node.get('text') for node in root.iter('node') if node.get('text')}

def header():
    matches = re.findall(r'\| (\d+)×(\d+) \| (\d+)×(\d+) chars', output())
    return tuple(map(int, matches[-1])) if matches else None

def mouse(code, col, row, up=False):
    os.write(master, f'\x1b[<{code};{col+1};{row+1}{"m" if up else "M"}'.encode())

def tap_text(root, text):
    node = next(node for node in root.iter('node') if node.get('text') == text)
    x0, y0, x1, y1 = map(int, re.findall(r'\d+', node.get('bounds')))
    screen = root.find('node')
    _, _, width, height = map(int, re.findall(r'\d+', screen.get('bounds')))
    _, _, cols, rows = header()
    x = (160 - cols) // 2
    y = 1 + (60 - rows) // 2
    col = x + int((x0 + x1) / 2 * cols / width)
    row = y + int((y0 + y1) / 2 * rows / height)
    mouse(0, col, row)
    time.sleep(0.12)
    mouse(0, col, row, up=True)
    time.sleep(0.7)

try:
    adb('shell', 'am', 'start', '-f', '0x10008000', '-a', 'android.settings.SETTINGS')
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH', 62, 160, 0, 0))
    before_term = termios.tcgetattr(slave)
    process = subprocess.Popen([str(args.binary), '--serial', args.serial, '--no-color'], stdin=slave, stdout=slave, stderr=slave, start_new_session=True)
    reader = threading.Thread(target=drain, daemon=True)
    reader.start()
    wait_for(lambda: header() is not None, 'live ASCII video')
    root = ui()
    tap_text(root, 'Network & internet')
    after_tap = texts(ui())
    assert 'Internet' in after_tap, 'Terminal click did not open Network & internet'
    print('PASS terminal click -> Android tap', flush=True)
    os.write(master, b'b')
    time.sleep(0.5)
    root = ui()
    assert 'Search Settings' in texts(root), 'Back shortcut failed'
    print('PASS Back shortcut', flush=True)
    before_texts = texts(root)
    _, _, cols, rows = header()
    col = 80
    start_row, end_row = 1 + rows * 4 // 5, 1 + rows // 4
    mouse(0, col, start_row)
    for row in range(start_row - 1, end_row, -1):
        mouse(32, col, row)
        time.sleep(0.02)
    mouse(0, col, end_row, up=True)
    time.sleep(0.6)
    after_swipe = ui()
    assert texts(after_swipe) != before_texts, 'Terminal drag did not scroll Settings'
    print('PASS terminal drag -> Android swipe', flush=True)
    os.write(master, b'h')
    time.sleep(0.6)
    assert re.search(r'(?:topResumedActivity|mResumedActivity)=.*launcher', adb('shell', 'dumpsys', 'activity', 'activities'), re.IGNORECASE), 'Home shortcut failed'
    print('PASS Home shortcut', flush=True)
    adb('shell', 'am', 'start', '-a', 'android.settings.SETTINGS')
    time.sleep(0.5)
    tap_text(ui(), 'Search Settings')
    search_ui = ui()
    assert any(n.get('class') == 'android.widget.EditText' and n.get('focused') == 'true' for n in search_ui.iter('node')), 'Search field is not focused'
    os.write(master, b'tbluetooth\r')
    time.sleep(0.5)
    typed_ui = ui()
    assert any(node.get('class') == 'android.widget.EditText' and node.get('text') == 'bluetooth' for node in typed_ui.iter('node')), 'Text entry failed'
    print('PASS terminal text -> Android search', flush=True)
    adb('shell', 'settings', 'put', 'system', 'accelerometer_rotation', '0')
    adb('shell', 'settings', 'put', 'system', 'user_rotation', '1')
    wait_for(lambda: header() and header()[0] > header()[1], 'rotation -> landscape geometry')
    process.send_signal(signal.SIGTERM)
    process.wait(timeout=10)
    reader.join(timeout=2)
    assert process.returncode == 0, output()[-1000:]
    assert termios.tcgetattr(slave) == before_term, 'Terminal attributes were not restored'
    assert '\x1b[?1049l' in output(), 'Alternate screen was not restored'
    assert adb('forward', '--list') == initial_forward, 'Session left an adb tunnel'
    assert not re.search(r'ascii-scrcpy-[0-9a-f]{8}\.jar', adb('shell', 'ls', '/data/local/tmp')), 'Session left its server jar'
    print('PASS SIGTERM restores terminal and cleans session', flush=True)
finally:
    if process and process.poll() is None:
        process.send_signal(signal.SIGTERM)
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
    for key, value in rotation.items():
        if value == 'null':
            adb('shell', 'settings', 'delete', 'system', key)
        else:
            adb('shell', 'settings', 'put', 'system', key, value)
    adb('shell', 'rm', '-f', remote_xml)
    if initial_component:
        adb('shell', 'am', 'start', '-n', initial_component)
    for fd in [master, slave]:
        if fd is not None:
            os.close(fd)

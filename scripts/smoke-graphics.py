#!/usr/bin/env python3
"""Exercise graphics negotiation, frame output, mode switching and cleanup in a PTY."""
import argparse
import fcntl
import os
import pty
import select
import signal
import struct
import subprocess
import termios
import time

parser = argparse.ArgumentParser()
parser.add_argument('--binary', required=True)
parser.add_argument('--case', choices=['supported', 'unsupported', 'signal'], default='supported')
args = parser.parse_args()
master, slave = pty.openpty()
fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH', 40, 100, 1000, 800))
original = termios.tcgetattr(slave)
proc = subprocess.Popen([args.binary, '--demo', '--render', 'graphics', '--max-fps', '5'], stdin=slave, stdout=slave, stderr=slave, start_new_session=True)
wire = bytearray()
try:
    deadline = time.monotonic() + 8
    replied = False
    switched = False
    quit_sent = False
    while time.monotonic() < deadline:
        if select.select([master], [], [], .1)[0]:
            wire.extend(os.read(master, 65536))
        if not replied and b'\x1b[c' in wire:
            # Coalesced replies catch polling a buffered stdin one byte at a time.
            if args.case == 'supported':
                os.write(master, b'\x1b_Gi=31;OK\x1b\\\x1b[?1;2c')
            elif args.case == 'unsupported':
                os.write(master, b'\x1b[?1;2c')
            else:
                proc.send_signal(signal.SIGTERM)
            replied = True
        if not switched and b'a=T,f=100' in wire:
            os.write(master, b'a')
            switched = True
        if switched and not quit_sent and b'a=d,d=I,i=31' in wire:
            os.write(master, b'q')
            quit_sent = True
        if proc.poll() is not None:
            break
    if args.case == 'supported':
        assert proc.poll() == 0, bytes(wire[-2000:])
        assert b'a=T,f=100' in wire, 'No graphics frame transmitted'
        assert b'\x1b[?1049l' in wire, 'Alternate screen not restored'
    else:
        assert proc.poll() not in (None, 0), bytes(wire[-2000:])
        assert b'a=T,f=100' not in wire, 'Unexpected image on a failed probe'
    restored = termios.tcgetattr(slave)
    # macOS may mark pending canonical input for reprocessing; this is not raw mode.
    restored[3] &= ~getattr(termios, 'PENDIN', 0)
    original[3] &= ~getattr(termios, 'PENDIN', 0)
    assert restored == original, f'Raw mode not restored: {original!r} -> {restored!r}'
    print(f'PASS: graphics terminal case {args.case}; terminal settings restored')

finally:
    if proc.poll() is None:
        proc.terminate()
        proc.wait(timeout=5)
    os.close(master)
    os.close(slave)

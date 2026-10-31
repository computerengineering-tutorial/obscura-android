#!/usr/bin/env python3
"""
Throttled logger for CI commands.
Prints only the latest (last) line of output at specified intervals (default: 30s / 0.5 min),
preventing GitHub Actions web UI freezes while providing clean, continuous progress updates.
Preserves the last N lines on failure and exits with the target command's exact exit code.
"""

import argparse
import os
import signal
import subprocess
import sys
import threading
import time
from collections import deque


def format_size(num_bytes):
    for unit in ["B", "KB", "MB", "GB"]:
        if num_bytes < 1024.0:
            return f"{num_bytes:.1f} {unit}"
        num_bytes /= 1024.0
    return f"{num_bytes:.1f} TB"


def run_throttled(cmd_args, interval=30.0, label="Task", watch_file=None, tail_lines=100, is_shell=False, log_file=None):
    start_time = time.time()
    last_line = ""
    last_printed_status = None
    line_lock = threading.Lock()
    recent_lines = deque(maxlen=tail_lines)
    stop_event = threading.Event()
    lbl = label or "Task"
    log_fp = None
    if log_file:
        try:
            log_dir = os.path.dirname(log_file)
            if log_dir:
                os.makedirs(log_dir, exist_ok=True)
            log_fp = open(log_file, "w", encoding="utf-8", errors="replace")
        except Exception as e:
            print(f"⚠️ Could not open log file {log_file}: {e}", file=sys.stderr)

    if cmd_args and cmd_args[0] == "--":
        cmd_args = cmd_args[1:]

    if is_shell:
        cmd_to_run = cmd_args[0] if isinstance(cmd_args, list) else cmd_args
    else:
        cmd_to_run = cmd_args

    print(f"🚀 [{lbl}] Started (Log interval: {interval}s). Real-time log flooding suppressed.", flush=True)

    proc = subprocess.Popen(
        cmd_to_run,
        shell=is_shell,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        bufsize=1,
    )

    def sig_handler(signum, frame):
        if proc.poll() is None:
            try:
                proc.terminate()
            except Exception:
                pass
        sys.exit(130 if signum == signal.SIGINT else 143)

    signal.signal(signal.SIGTERM, sig_handler)
    signal.signal(signal.SIGINT, sig_handler)

    def reader():
        nonlocal last_line
        try:
            for line in proc.stdout:
                if log_fp:
                    log_fp.write(line)
                    log_fp.flush()
                cleaned = line.rstrip()
                if cleaned:
                    with line_lock:
                        last_line = cleaned
                        recent_lines.append(cleaned)
        except Exception:
            pass
        finally:
            if log_fp:
                try:
                    log_fp.close()
                except Exception:
                    pass

    t_reader = threading.Thread(target=reader, daemon=True)
    t_reader.start()

    def get_status_str():
        status = ""
        with line_lock:
            if last_line:
                status = last_line
        if watch_file and os.path.exists(watch_file):
            try:
                sz = os.path.getsize(watch_file)
                watch_info = f"[{os.path.basename(watch_file)}: {format_size(sz)}]"
                status = f"{watch_info} {status}".strip()
            except Exception:
                pass
        return status or "(running...)"

    last_tick = time.time()
    while proc.poll() is None:
        stop_event.wait(timeout=1.0)
        now = time.time()
        if now - last_tick >= interval:
            elapsed = int(now - start_time)
            mins, secs = divmod(elapsed, 60)
            status = get_status_str()
            print(f"[{mins:02d}:{secs:02d}] {status}", flush=True)
            last_printed_status = status
            last_tick = now

    proc.wait()
    stop_event.set()
    t_reader.join(timeout=2.0)
    elapsed = int(time.time() - start_time)
    mins, secs = divmod(elapsed, 60)

    final_status = get_status_str()
    if proc.returncode == 0:
        if final_status and final_status != last_printed_status and final_status != "(running...)":
            print(f"[{mins:02d}:{secs:02d}] {final_status}", flush=True)
        print(f"✅ [{lbl}] Completed successfully in {mins}m {secs}s.", flush=True)
        return 0
    else:
        print(f"\n❌ [{lbl}] FAILED with code {proc.returncode} after {mins}m {secs}s!", flush=True)
        if log_file and os.path.exists(log_file):
            print(f"📄 Full raw logs captured at: {log_file} ({format_size(os.path.getsize(log_file))})", flush=True)
            try:
                error_lines = []
                with open(log_file, "r", encoding="utf-8", errors="replace") as f:
                    for l in f:
                        if any(marker in l for marker in ["error:", "FAILED:", "fatal error:", "undefined reference", "cargo:warning"]):
                            error_lines.append(l.rstrip())
                if error_lines:
                    print(f"====== First {min(len(error_lines), 25)} Detected Compiler Errors ======", flush=True)
                    for err in error_lines[:25]:
                        print(err, flush=True)
                    if len(error_lines) > 25:
                        print(f"... and {len(error_lines) - 25} more errors found in {log_file}", flush=True)
                    print("================================================================", flush=True)
            except Exception:
                pass
        print(f"====== Last {len(recent_lines)} lines of logs ======", flush=True)
        for l in list(recent_lines):
            print(l, flush=True)
        print("==========================================", flush=True)
        return proc.returncode


def run_stdin_throttled(interval=30.0, label="Task", watch_file=None, tail_lines=100, log_file=None):
    start_time = time.time()
    last_line = ""
    last_printed_status = None
    line_lock = threading.Lock()
    recent_lines = deque(maxlen=tail_lines)
    stop_event = threading.Event()
    lbl = label or "Task"
    log_fp = None
    if log_file:
        try:
            log_dir = os.path.dirname(log_file)
            if log_dir:
                os.makedirs(log_dir, exist_ok=True)
            log_fp = open(log_file, "w", encoding="utf-8", errors="replace")
        except Exception as e:
            print(f"⚠️ Could not open log file {log_file}: {e}", file=sys.stderr)

    print(f"🚀 [{lbl}] Started reading stdin (Log interval: {interval}s).", flush=True)

    def reader():
        nonlocal last_line
        try:
            for line in iter(sys.stdin.readline, ""):
                if log_fp:
                    log_fp.write(line)
                    log_fp.flush()
                cleaned = line.rstrip()
                if cleaned:
                    with line_lock:
                        last_line = cleaned
                        recent_lines.append(cleaned)
        except Exception:
            pass
        finally:
            if log_fp:
                try:
                    log_fp.close()
                except Exception:
                    pass
        stop_event.set()

    t_reader = threading.Thread(target=reader, daemon=True)
    t_reader.start()

    def get_status_str():
        status = ""
        with line_lock:
            if last_line:
                status = last_line
        if watch_file and os.path.exists(watch_file):
            try:
                sz = os.path.getsize(watch_file)
                watch_info = f"[{os.path.basename(watch_file)}: {format_size(sz)}]"
                status = f"{watch_info} {status}".strip()
            except Exception:
                pass
        return status or "(running...)"

    last_tick = time.time()
    while not stop_event.is_set():
        stop_event.wait(timeout=1.0)
        now = time.time()
        if now - last_tick >= interval:
            elapsed = int(now - start_time)
            mins, secs = divmod(elapsed, 60)
            status = get_status_str()
            print(f"[{mins:02d}:{secs:02d}] {status}", flush=True)
            last_printed_status = status
            last_tick = now

    t_reader.join(timeout=2.0)
    elapsed = int(time.time() - start_time)
    mins, secs = divmod(elapsed, 60)

    final_status = get_status_str()
    if final_status and final_status != last_printed_status and final_status != "(running...)":
        print(f"[{mins:02d}:{secs:02d}] {final_status}", flush=True)
    print(f"✅ [{lbl}] Stream ended in {mins}m {secs}s.", flush=True)
    return 0


def main():
    parser = argparse.ArgumentParser(
        description="Throttle high-frequency logs in CI to 1 line every N seconds."
    )
    parser.add_argument(
        "-i", "--interval", type=float, default=30.0, help="Interval in seconds between logs (default: 30.0)"
    )
    parser.add_argument("-l", "--label", type=str, default="Task", help="Label for the task")
    parser.add_argument(
        "-w", "--watch-file", type=str, default=None, help="Optional file to monitor size during execution"
    )
    parser.add_argument(
        "-n", "--tail-lines", type=int, default=100, help="Lines of output to show on failure (default: 100)"
    )
    parser.add_argument(
        "--log-file", type=str, default=None, help="File path to save full unfiltered build logs"
    )
    parser.add_argument(
        "-c", "--shell-cmd", type=str, default=None, help="Execute string command inside bash shell"
    )
    parser.add_argument("cmd", nargs=argparse.REMAINDER, help="Command and arguments to execute")

    args = parser.parse_args()

    if args.shell_cmd:
        code = run_throttled(
            args.shell_cmd,
            interval=args.interval,
            label=args.label,
            watch_file=args.watch_file,
            tail_lines=args.tail_lines,
            is_shell=True,
            log_file=args.log_file,
        )
        sys.exit(code)
    elif args.cmd and (len(args.cmd) > 1 or (len(args.cmd) == 1 and args.cmd[0] != "--")):
        cmd_args = args.cmd[1:] if args.cmd[0] == "--" else args.cmd
        code = run_throttled(
            cmd_args,
            interval=args.interval,
            label=args.label,
            watch_file=args.watch_file,
            tail_lines=args.tail_lines,
            is_shell=False,
            log_file=args.log_file,
        )
        sys.exit(code)
    else:
        code = run_stdin_throttled(
            interval=args.interval,
            label=args.label,
            watch_file=args.watch_file,
            tail_lines=args.tail_lines,
            log_file=args.log_file,
        )
        sys.exit(code)


if __name__ == "__main__":
    main()

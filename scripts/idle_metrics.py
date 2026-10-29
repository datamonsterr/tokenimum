#!/usr/bin/env python3
"""Measure daemon CPU over a 30-second idle window and export idle metrics."""
import json
import os
import pathlib
import sys
import time


def cpu_percent(start, end, ticks_per_second, seconds):
    if end < start or ticks_per_second <= 0 or seconds <= 0:
        raise ValueError('invalid CPU measurement')
    return (end - start) / ticks_per_second / seconds * 100


def regression_cpu(value, ticks_per_second, window_seconds):
    """Floor the comparison at one tick, the counter's resolution for the window."""
    if value < 0 or ticks_per_second <= 0 or window_seconds <= 0:
        raise ValueError('invalid CPU measurement')
    return max(value, 100 / (ticks_per_second * window_seconds))


def below_budget(value, limit):
    return 0 <= value < limit


def ticks(pid):
    stat = pathlib.Path(f'/proc/{pid}/stat').read_text(encoding='utf-8')
    fields = stat[stat.rfind(')') + 2:].split()
    return int(fields[11]) + int(fields[12])


def main():
    pid, label, samples_path, output_path = sys.argv[1:]
    start = ticks(pid)
    start_time = time.monotonic()
    time.sleep(30)
    end = ticks(pid)
    elapsed = time.monotonic() - start_time
    cpu = cpu_percent(start, end, os.sysconf('SC_CLK_TCK'), elapsed)
    rss = max(int(s) for s in pathlib.Path(samples_path).read_text(encoding='utf-8').splitlines()) / 1024
    print(f'{label}: idle CPU {cpu:.4f}% over {elapsed:.2f}s (limit < 1%)')
    metrics = [{'name': f'idle {label} CPU', 'unit': '%', 'value': regression_cpu(cpu, os.sysconf('SC_CLK_TCK'), 30)},
               {'name': f'idle {label} memory', 'unit': 'MiB', 'value': rss}]
    pathlib.Path(output_path).write_text(json.dumps(metrics) + '\n', encoding='utf-8')
    return 0 if below_budget(cpu, 1.0) else 1


if __name__ == '__main__':
    sys.exit(main())

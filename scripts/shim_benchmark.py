#!/usr/bin/env python3
"""Check and export a shim's hyperfine wall-clock p50 and p99."""
import json
import pathlib
import sys
from check_hook_latency import summarize


def convert(export, host):
    stats = summarize(export)
    return [{'name': f'shim {host} {key}', 'unit': 'ms', 'value': stats[key]}
            for key in ('p50', 'p99')]


def within_budget(metrics):
    return metrics[0]['value'] < 15 and metrics[1]['value'] < 40


def main():
    host, source, destination = sys.argv[1:]
    metrics = convert(json.loads(pathlib.Path(source).read_text(encoding='utf-8')), host)
    print(json.dumps(metrics))
    pathlib.Path(destination).write_text(json.dumps(metrics) + '\n', encoding='utf-8')
    return 0 if within_budget(metrics) else 1


if __name__ == '__main__':
    sys.exit(main())

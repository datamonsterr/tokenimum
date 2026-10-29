#!/usr/bin/env python3
"""Export hook round-trip percentiles for github-action-benchmark."""
import json
import sys

from check_hook_latency import summarize


def convert(export):
    stats = summarize(export)
    return [{'name': f'hook round trip {key}', 'unit': 'ms', 'value': stats[key]}
            for key in ('p50', 'p99')]


def main():
    with open(sys.argv[1], encoding='utf-8') as source:
        results = convert(json.load(source))
    with open(sys.argv[2], 'w', encoding='utf-8') as output:
        json.dump(results, output)
        output.write('\n')


if __name__ == '__main__':
    main()

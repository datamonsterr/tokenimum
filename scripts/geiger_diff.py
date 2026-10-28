#!/usr/bin/env python3
"""Compare cargo-geiger JSON reports against the reviewed crate allowlist."""
import json
import pathlib
import re
import sys

allowlist = pathlib.Path('docs/unsafe-allowlist.md').read_text()
allowed = set(re.findall(r'^\|\s*([a-zA-Z0-9_-]+)\s*\|', allowlist, re.MULTILINE))
if len(sys.argv) < 2:
    sys.exit('usage: geiger_diff.py REPORT.json [...]')
found = set()
for filename in sys.argv[1:]:
    with open(filename, encoding='utf-8') as report_file:
        report = json.load(report_file)
    if report['packages_without_metrics'] or report['used_but_not_scanned_files']:
        sys.exit(f'{filename}: incomplete geiger scan')
    for entry in report['packages']:
        counts = entry['unsafety']['used']
        if any(value['unsafe_'] > 0 for value in counts.values()):
            found.add(entry['package']['id']['name'])
missing = found - allowed
if missing:
    sys.exit('Unreviewed unsafe dependencies: ' + ', '.join(sorted(missing)))
print('All used unsafe dependencies are allowlisted.')

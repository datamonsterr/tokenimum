#!/usr/bin/env python3
"""Compare cargo-geiger JSON reports against the reviewed crate allowlist."""
import json
import pathlib
import re
import sys


def registry_package(filename):
    """Identify a Cargo registry package even if it has no report entry."""
    match = re.search(r'(?:^|/)registry/src/[^/]+/([A-Za-z0-9_-]+)-\d+\.\d+\.\d+(?:[-+][^/]*)?/', filename)
    return match.group(1) if match else None


def show(title, names):
    print(f'{title}:')
    for name in sorted(names):
        print(f'  {name}')
    if not names:
        print('  (none)')


if len(sys.argv) < 2:
    sys.exit('usage: geiger_diff.py REPORT.json [...]')
allowlist = pathlib.Path('docs/unsafe-allowlist.md').read_text(encoding='utf-8')
allowed = set(re.findall(r'^\|\s*([a-zA-Z0-9_-]+)\s*\|', allowlist, re.MULTILINE)) - {'crate'}
used_unsafe, unscanned, seen = set(), set(), set()
for filename in sys.argv[1:]:
    with open(filename, encoding='utf-8') as report_file:
        report = json.load(report_file)
    for entry in report['packages']:
        name = entry['package']['id']['name']
        seen.add(name)
        if any(value['unsafe_'] > 0 for value in entry['unsafety']['used'].values()):
            used_unsafe.add(name)
    for package_id in report['packages_without_metrics']:
        unscanned.add(package_id['name'])
        seen.add(package_id['name'])
    for path in report['used_but_not_scanned_files']:
        name = registry_package(path)
        # An unknown owner must never silently pass the allowlist check.
        unscanned.add(name if name else f'<unknown owner: {path}>')
        if name:
            seen.add(name)

missing = (used_unsafe | unscanned) - allowed
show('Used-unsafe crates', used_unsafe)
show('Unscanned crates', unscanned)
show('Missing from allowlist', missing)
show('Warning: allowlist rows not seen in any report', allowed - seen)
sys.exit(1 if missing else 0)

#!/usr/bin/env python3
"""Compare cargo-geiger JSON reports against the allowlist and count baseline."""
import argparse
import json
import pathlib
import re
import sys


def registry_package(filename):
    """Identify a registry package or Cargo build-output owner."""
    match = re.search(r'(?:^|/)registry/src/[^/]+/([A-Za-z0-9_-]+)-\d+\.\d+\.\d+(?:[-+][^/]*)?/', filename)
    if match:
        return match.group(1)
    match = re.search(r'(?:^|/)target/[^/]+/build/([A-Za-z0-9_][A-Za-z0-9_-]*)-[0-9a-f]{16}/out/', filename)
    return match.group(1) if match else None


def show(title, names):
    print(f'{title}:')
    for name in sorted(names):
        print(f'  {name}')
    if not names:
        print('  (none)')


parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--write-baseline', type=pathlib.Path, metavar='PATH')
parser.add_argument('reports', nargs='+', type=pathlib.Path)
args = parser.parse_args()
allowlist = pathlib.Path('docs/unsafe-allowlist.md').read_text(encoding='utf-8')
allowed = set(re.findall(r'^\|\s*([a-zA-Z0-9_][a-zA-Z0-9_-]*)\s*\|', allowlist, re.MULTILINE)) - {'crate'}
used_unsafe, unscanned, seen = set(), set(), set()
counts = {}
for filename in args.reports:
    with filename.open(encoding='utf-8') as report_file:
        report = json.load(report_file)
    for entry in report['packages']:
        package_id = entry['package']['id']
        name, version = package_id['name'], str(package_id['version'])
        seen.add(name)
        used = entry['unsafety']['used']
        count = sum(used[key]['unsafe_'] for key in ('functions', 'exprs', 'item_impls', 'item_traits', 'methods'))
        if count:
            used_unsafe.add(name)
        # Reports overlap; keep the largest count for a name without double counting.
        candidate = {'version': version, 'used_unsafe': count}
        if name not in counts or (count, version) > (counts[name]['used_unsafe'], counts[name]['version']):
            counts[name] = candidate
    for package_id in report['packages_without_metrics']:
        unscanned.add(package_id['name'])
        seen.add(package_id['name'])
    for path in report['used_but_not_scanned_files']:
        name = registry_package(path)
        unscanned.add(name if name else f'<unknown owner: {path}>')
        if name:
            seen.add(name)

missing = (used_unsafe | unscanned) - allowed
show('Used-unsafe crates', used_unsafe)
show('Unscanned crates', unscanned)
show('Missing from allowlist', missing)
show('Warning: allowlist rows not seen in any report', allowed - seen)
if args.write_baseline:
    args.write_baseline.write_text(json.dumps(dict(sorted(counts.items())), indent=2) + '\n', encoding='utf-8')
    print(f'Wrote baseline: {args.write_baseline}')
    sys.exit(1 if missing else 0)

baseline_path = pathlib.Path('docs/geiger-baseline.json')
if not baseline_path.exists():
    print('WARNING: GEIGER BASELINE MISSING; UNSAFE COUNT COMPARISON IS DISABLED!', file=sys.stderr)
    sys.exit(1 if missing else 0)
baseline = json.loads(baseline_path.read_text(encoding='utf-8'))
violations = []
for name, current in sorted(counts.items()):
    previous = baseline.get(name)
    if previous is None:
        if current['used_unsafe']:
            violations.append(f"{name}: {current['used_unsafe']} used unsafe, no baseline")
    elif current['used_unsafe'] > previous['used_unsafe']:
        violations.append(f"{name}: {previous['used_unsafe']} -> {current['used_unsafe']} used unsafe")
    elif current['version'] != previous['version']:
        print(f"Notice: {name} version {previous['version']} -> {current['version']} (used unsafe {previous['used_unsafe']} -> {current['used_unsafe']})")
show('Unsafe count increases or missing baseline entries', violations)
sys.exit(1 if missing or violations else 0)

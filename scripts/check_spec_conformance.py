#!/usr/bin/env python3
"""Check that the spec conformance tables have valid local evidence."""
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parent.parent
HEAD = re.compile(r"^(#{2,3})\s+(.+?)\s*$", re.M)
LINK = re.compile(r"\[[^\]]+\]\(([^)]+)\)")


def cells(line):
    return [part.strip().replace(r'\|', '|') for part in re.split(r'(?<!\\)\|', line.strip().strip('|'))]


def check(spec, audit, root):
    errors = []
    headings = [m.group(2) for m in HEAD.finditer(spec)]
    sections = list(HEAD.finditer(audit))
    by_heading = {m.group(2): audit[m.end():sections[i + 1].start() if i + 1 < len(sections) else len(audit)]
                  for i, m in enumerate(sections)}
    ids = set()
    for heading in headings:
        body = by_heading.get(heading, '')
        lines = body.splitlines()
        header = next((i for i, line in enumerate(lines) if cells(line) == ['ID', 'Requirement', 'Status', 'Evidence']), None)
        if header is None or header + 1 >= len(lines) or not re.fullmatch(r'[\s|:\-]+', lines[header + 1]):
            errors.append(f'{heading}: missing table')
            continue
        count = 0
        for line in lines[header + 2:]:
            if not line.strip().startswith('|'):
                break
            row = cells(line)
            if len(row) != 4:
                errors.append(f'{heading}: malformed row: {line}')
                continue
            ident, requirement, status, evidence = row
            count += 1
            if not ident or ident in ids:
                errors.append(f'{heading}: missing or duplicate ID {ident}')
            ids.add(ident)
            if not requirement or status not in {'met', 'deferred', 'not applicable'}:
                errors.append(f'{ident}: invalid requirement or status')
            if not evidence or evidence in {'-', '—', 'TBD'}:
                errors.append(f'{ident}: missing evidence')
            links = LINK.findall(evidence)
            if not links:
                errors.append(f'{ident}: evidence needs a local file link')
            paths = links + ['../' + p for p in re.findall(r'`([^`]+)`', evidence)
                             if re.match(r'^(?:crates|docs|scripts|fixtures|shims|\.github)/', p)]
            for link in paths:
                path = link.split('#', 1)[0]
                if '://' in path or path.startswith('/') or not (root / 'docs' / path).is_file():
                    errors.append(f'{ident}: evidence file does not exist: {link}')
            if status == 'deferred' and not any((root / 'docs' / link.split('#', 1)[0]).resolve().is_relative_to((root / 'docs').resolve()) for link in links):
                errors.append(f'{ident}: deferred requires a docs link')
            if status == 'not applicable' and len(LINK.sub('', evidence).strip(' .;—-')) == 0:
                errors.append(f'{ident}: not applicable requires a reason')
        if not count:
            errors.append(f'{heading}: empty table')
    return errors


def main():
    errors = check((ROOT / 'SPEC.md').read_text(), (ROOT / 'docs/spec-conformance.md').read_text(), ROOT)
    for error in errors:
        print(error, file=sys.stderr)
    return bool(errors)


if __name__ == '__main__':
    sys.exit(main())

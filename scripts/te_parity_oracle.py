#!/usr/bin/env python3
# Portions ported from microsoft/TokenEconomizer, commit d0675a3:
# microskills/envfacts.py, lifecycle.py; loopguard/rules.py, state.py, turn.py.
# Copyright (c) Microsoft Corporation, Apache License 2.0 (see NOTICE).
"""Reproduce the TE oracle for the committed Claude/Codex logs, without importing Rust.

This deliberately scoped reference covers the fixture vocabulary: command-not-found
form repair, per-session confirmations, repeated Python errors, sed edits, and TE's
18-call free gate / strict 20-call rule / 2.5x envelope backstop. Unsupported native
records are ignored; the Rust test also checks per-turn call counts to catch drift.
Run with --check to verify outputs, or without arguments to regenerate them.
"""
import argparse
import json
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]


def text(value):
    if isinstance(value, str):
        return value
    if isinstance(value, list):
        return '\n'.join(text(part) for part in value)
    if isinstance(value, dict):
        if 'stdout' in value or 'stderr' in value:
            return '\n'.join(value.get(key, '') for key in ('stdout', 'stderr') if value.get(key))
        return text(value.get('text', value.get('content', value.get('output', ''))))
    return ''


def read_calls(host, path):
    """Independent native-log decoding; no tkm normalized output is an input."""
    turns, pending, seen = [], {}, set()
    for line in path.read_text().splitlines():
        row = json.loads(line)
        if host == 'claude':
            if row.get('isSidechain') or row.get('isMeta') or row.get('isCompactSummary'):
                continue
            message = row.get('message', {})
            content = message.get('content', [])
            if row.get('type') == 'user' and isinstance(content, str):
                if content.strip():
                    turns.append([])
            elif isinstance(content, list):
                for block in content:
                    if block.get('type') == 'tool_use' and block['id'] not in seen:
                        seen.add(block['id'])
                        args = block['input']
                        call = dict(tool=block['name'], command=args.get('command', ''), output='', failed=False,
                                    edit=Path(args.get('file_path', '')).name if block['name'] in ('Write', 'Edit') else '')
                        turns[-1].append(call)
                        pending[block['id']] = call
                    elif block.get('type') == 'tool_result':
                        call = pending.pop(block.get('tool_use_id'), None)
                        if call is not None:
                            call.update(output=text(block.get('content')), failed=block.get('is_error', False))
        else:
            payload = row.get('payload', {})
            if row.get('type') != 'event_msg':
                continue
            if payload.get('type') == 'task_started':
                turns.append([])
            if payload.get('type') != 'item_completed':
                continue
            item = payload['item']
            if item['type'] == 'CommandExecution':
                command = item['command']
                if isinstance(command, list):
                    command = command[-1]
                turns[-1].append(dict(tool='Bash', command=command, output=item.get('aggregated_output', ''),
                                      failed=item.get('exit_code', 0) != 0, edit=''))
            elif item['type'] == 'FileChange':
                turns[-1].append(dict(tool='apply_patch', command='', output='', failed=False,
                                      edit=Path(next(iter(item['changes']))).name))
    return turns


def te_rules(turns):
    calls = [call for turn in turns for call in turn]
    ids = []
    # TE envfacts: a failing bare runner, followed by a successful different form
    # with the same intent, contributes once per session, never once per call.
    for index, call in enumerate(calls):
        if call['command'] == 'pytest -q' and 'command not found' in call['output']:
            if any(later['command'] == 'python -m pytest -q' and 'passed' in later['output']
                   for later in calls[index + 1:]):
                ids.append('form_repair:run_tests:pytest')
    ids = sorted(set(ids))
    loops = []
    for turn in turns:
        errors, repeated, edits, signatures = 0, 0, {}, set()
        judges, backstops = [], []
        for number, call in enumerate(turn, 1):
            edit = call['edit']
            if call['command'].startswith('sed -i '):
                edit = Path(call['command'].split()[-1]).name
            if edit:
                edits[edit] = edits.get(edit, 0) + 1
            match = re.search(r'(\w*Error)(?::|\b)', call['output'])
            if match and (call['failed'] or call['tool'] not in ('Read', 'read')):
                errors += 1
                signature = re.sub(r'\d+', '#', call['output'])
                repeated += signature in signatures
                signatures.add(signature)
            stuck = (number >= 20 and (max(edits.values(), default=0) >= 4 or len(edits) >= 3)
                     and errors >= 6 and repeated / max(errors, 1) >= .20)
            if number > 2.5 * 8 and stuck:
                backstops.append(number)
                # The fixtures end here; segment reset semantics are tested in the ported suite.
                break
            if not judges and number >= 18 and (repeated >= 1 or errors >= 8):
                judges.append(number)
        loops.append(dict(judge_calls=judges, backstop_calls=backstops))
    return dict(calls_per_turn=[len(turn) for turn in turns], candidate_ids=ids,
                te_active_after_sessions=[ids if n >= 3 else [] for n in range(1, 5)],
                code_only_active_after_sessions=[ids if n >= 4 else [] for n in range(1, 5)], loops=loops)


def generate(host):
    original = sorted((ROOT / 'fixtures' / host / 'sessions').rglob('*.jsonl'))[0]
    paths = [original, ROOT / 'fixtures' / host / 'parity/behavior.jsonl']
    return dict(te_commit='d0675a3', rules='ported TE rules; scripts/te_parity_oracle.py',
                cases=[dict(source=str(path.relative_to(ROOT)), **te_rules(read_calls(host, path))) for path in paths])


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args(argv)
    for host in ('claude', 'codex'):
        path = ROOT / 'fixtures/golden' / f'parity_{host}_oracle.json'
        result = generate(host)
        if args.check:
            if json.loads(path.read_text()) != result:
                parser.exit(1, f'{path}: stale TE oracle\n')
        else:
            path.write_text(json.dumps(result, indent=2) + '\n')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())

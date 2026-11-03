#!/usr/bin/env python3
"""Compare Criterion (bencher format) samples of a PR head against its base.

Usage: compare_bench.py [--threshold 1.20] [--summary FILE] --head F... --base F...

Each file is one sample (one full bench pass). Head and base are measured on the same runner,
interleaved. Per benchmark the medians are compared. A benchmark regresses only when
head/base is above the threshold AND above 1 + noise, where noise is the larger relative
spread (max-min over median) of the head or base samples.

Exit: 0 pass, 1 regression, 2 usage or input error.
"""
import argparse
import re
import sys

LINE = re.compile(r"^test (?P<name>\S+)\s+\.\.\. bench:\s+(?P<mean>[\d,]+) ns/iter")


def collect(texts):
    out = {}
    for text in texts:
        for line in text.splitlines():
            m = LINE.match(line.strip())
            if m:
                out.setdefault(m["name"], []).append(int(m["mean"].replace(",", "")))
    return out


def median(values):
    s = sorted(values)
    n = len(s)
    return s[n // 2] if n % 2 else (s[n // 2 - 1] + s[n // 2]) / 2


def spread(values):
    med = median(values)
    return (max(values) - min(values)) / med if med else 0.0


def compare(head_texts, base_texts, threshold):
    head, base = collect(head_texts), collect(base_texts)
    rows = []
    for name in sorted(set(head) | set(base)):
        if name not in base:
            rows.append({"name": name, "status": "new"})
        elif name not in head:
            rows.append({"name": name, "status": "removed"})
        else:
            h, b = median(head[name]), median(base[name])
            ratio = h / b
            noise = max(spread(head[name]), spread(base[name]))
            if ratio > threshold and ratio > 1 + noise:
                status = "regression"
            elif ratio > threshold:
                status = "noisy"
            else:
                status = "ok"
            rows.append({"name": name, "status": status, "head": h, "base": b,
                         "ratio": ratio, "noise": noise,
                         "samples": f"{len(head[name])}/{len(base[name])}"})
    return rows


def render(rows, threshold):
    lines = ["| benchmark | head ns | base ns | ratio | noise | n | status |",
             "|---|---|---|---|---|---|---|"]
    for r in rows:
        if "ratio" in r:
            lines.append(f"| {r['name']} | {r['head']:.0f} | {r['base']:.0f} | {r['ratio']:.2f} | "
                         f"{r['noise']:.2f} | {r['samples']} | {r['status']} |")
        else:
            lines.append(f"| {r['name']} | | | | | | {r['status']} |")
    return f"Criterion head vs base (threshold {threshold:.2f}x, medians)\n\n" + "\n".join(lines) + "\n"


def main(argv):
    p = argparse.ArgumentParser(prog=argv[0], description=__doc__)
    p.add_argument("--threshold", type=float, default=1.20)
    p.add_argument("--summary")
    p.add_argument("--head", nargs="+", required=True)
    p.add_argument("--base", nargs="+", required=True)
    try:
        args = p.parse_args(argv[1:])
        texts = {}
        for key in ("head", "base"):
            texts[key] = []
            for path in getattr(args, key):
                with open(path, encoding="utf-8") as source:
                    texts[key].append(source.read())
    except OSError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 2
    except SystemExit:
        return 2
    rows = compare(texts["head"], texts["base"], args.threshold)
    report = render(rows, args.threshold)
    print(report)
    if args.summary:
        with open(args.summary, "a", encoding="utf-8") as out:
            out.write(report)
    bad = [r["name"] for r in rows if r["status"] == "regression"]
    if bad:
        print(f"FAIL: regression beyond noise: {', '.join(bad)}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))

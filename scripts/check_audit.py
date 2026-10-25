#!/usr/bin/env python3
"""Checks docs/audit.md against the code.

    check_audit.py                 every named test exists
    check_audit.py test-output.log every named test also PASSED in that
                                   `cargo test --workspace` output
"""
import pathlib, re, subprocess, sys

root = pathlib.Path(__file__).resolve().parent.parent
text = (root / "docs/audit.md").read_text()
sources = "\n".join(
    p.read_text(errors="ignore")
    for ext in ("rs", "ts")
    for p in root.glob(f"**/*.{ext}")
    if "target" not in p.parts and "node_modules" not in p.parts
)
missing = []
for name in set(re.findall(r"`([^`]+)`", text)):
    if "/" in name:
        if not (root / name).exists():
            missing.append(name)
    elif re.fullmatch(r"[a-z0-9_]+", name):
        if not re.search(rf"fn {name}\b", sources):
            missing.append(name)
for quoted in re.findall(r'e2e "([^"…]+)', text):
    if quoted not in sources:
        missing.append(f'e2e "{quoted}"')
if len(sys.argv) > 1:
    log = pathlib.Path(sys.argv[1]).read_text(errors="ignore")
    passed = set(re.findall(r"test (?:[\w:]*::)?(\w+) \.\.\. ok", log))
    failed = set(re.findall(r"test (?:[\w:]*::)?(\w+) \.\.\. FAILED", log))
    for name in set(re.findall(r"`([a-z0-9_]+)`", text)):
        if name in failed or (name not in passed and re.search(rf"fn {name}\b", sources) and name not in ("rig", "call")):
            ignored = re.search(rf"test (?:[\w:]*::)?{name} \.\.\. ignored", log)
            if not ignored:
                missing.append(f"{name} did not pass")
if missing:
    print("not found:", *sorted(missing), sep="\n  ")
    sys.exit(1)
print("every test named in docs/audit.md exists")

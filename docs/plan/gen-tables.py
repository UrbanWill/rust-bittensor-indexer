#!/usr/bin/env python3
"""Regenerate the step table in README.md and the whole of learning-map.md
from the stage files, so the three cannot disagree.

Run from anywhere: python3 docs/plan/gen-tables.py
Pass --check to exit 1 when either file is out of date, without writing.
"""
import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
START, END = "<!-- steps:start -->", "<!-- steps:end -->"
STANDING = ("cargo fmt --all --check", "cargo clippy --workspace --all-targets -- -D warnings", "cargo test --workspace")
COMMANDS = ("cargo ", "curl ", "docker ", "git push", "git cliff", "gh ", "jq ", "scripts/", "env ", "unshare ", "node ", "npm ")

MAP_INTRO = """# Learning map

Every Rust and tokio concept in the [plan](README.md), in the order you meet it. Each row names the step that first uses the concept and one primary source. The step itself says why the project uses the concept there, and which mistake a JavaScript engineer tends to make with it.

Read the source when you reach the step, not before. A concept that a step does not list is one the plan assumes you know.

This file is generated from the stage files by `gen-tables.py`. Edit the step, not this file.
"""


def bullet(body: str, name: str) -> str:
    """Text of the top-level bullet `- **Name...**`, up to the next top-level bullet."""
    m = re.search(rf"^- \*\*{name}[^*]*\*\*(.*?)(?=^- \*\*|\Z)", body, re.S | re.M)
    return m.group(1) if m else ""


def who(body: str) -> str:
    text = bullet(body, "Who writes what")
    verbs = r"(writes?|records?|adds?|generates?|builds?|fills?|ports?|picks?|runs?)\b"
    human = re.search(rf"\b(You|Human) {verbs}", text) is not None
    agent = re.search(rf"\b([Tt]he agent|Agent) {verbs}", text) is not None
    if not text and "Brief for the agent" in body:
        return "agent"
    if human and agent:
        return "both"
    if human:
        return "human"
    return "agent"


def check(body: str) -> str:
    text = bullet(body, "Check")
    spans = re.findall(r"`([^`\n]+)`", text)
    fenced = re.findall(r"```\w*\n(.*?)```", text, re.S)
    for block in fenced:
        first = block.strip().splitlines()[0].strip().rstrip("\\").strip()
        spans.insert(0, first)
    for span in spans:
        if span.startswith(COMMANDS) and span not in STANDING:
            return f"`{span}`"
    return "see the step"


def concepts(body: str):
    text = bullet(body, "Concepts")
    out = []
    for m in re.finditer(r"^  - \*(.+?)\*\.?(.*?)(?=^  - \*|\Z)", text, re.S | re.M):
        src = re.search(r"Sources?: (.*)", m.group(2), re.S)
        source = " ".join(src.group(1).split()).rstrip(".") if src else "none given"
        out.append((m.group(1).rstrip("."), source))
    return out


def stages():
    for path in sorted(HERE.glob("0[1-8]-*.md")):
        text = path.read_text()
        title = re.match(r"# Stage (\d+)\. (.+)", text)
        steps = []
        parts = re.split(r"^## Step (\d+)\. `(.+?)`\s*$", text, flags=re.M)
        for i in range(1, len(parts), 3):
            body = re.split(r"^## ", parts[i + 2], flags=re.M)[0]
            steps.append((int(parts[i]), parts[i + 1], body))
        yield path.name, int(title.group(1)), title.group(2), steps


def render():
    rows, map_out, seen = [], [MAP_INTRO], set()
    expected = 1
    for name, num, title, steps in stages():
        stage_rows = []
        for n, subject, body in steps:
            if n != expected:
                sys.exit(f"step numbering breaks at {name}: found {n}, expected {expected}")
            expected += 1
            cs = concepts(body)
            names = ", ".join(c for c, _ in cs) or "none"
            rows.append(f"| {n} | [{num}]({name}) | `{subject}` | {who(body)} | {names} | {check(body)} |")
            for concept, source in cs:
                if concept not in seen:
                    seen.add(concept)
                    stage_rows.append(f"| {concept} | [Step {n}]({name}) | {source} |")
        if stage_rows:
            map_out.append(f"## Stage {num}. {title}\n\n| Concept | First used in | Primary source |\n|---|---|---|\n" + "\n".join(stage_rows) + "\n")
    table = "| Step | Stage | Commit subject | Who writes it | Concepts first used | Check |\n|---|---|---|---|---|---|\n" + "\n".join(rows)
    return table, "\n".join(map_out), expected - 1, len(seen)


def main():
    table, learning_map, steps, concept_count = render()
    readme_path, map_path = HERE / "README.md", HERE / "learning-map.md"
    readme = readme_path.read_text()
    if START not in readme or END not in readme:
        sys.exit(f"README.md needs the markers {START} and {END}")
    head, rest = readme.split(START, 1)
    _, tail = rest.split(END, 1)
    new_readme = f"{head}{START}\n{table}\n{END}{tail}"
    stale = [p.name for p, new in ((readme_path, new_readme), (map_path, learning_map)) if p.read_text() != new]
    if "--check" in sys.argv:
        if stale:
            sys.exit("out of date: " + ", ".join(stale))
        print(f"up to date: {steps} steps, {concept_count} concepts")
        return
    readme_path.write_text(new_readme)
    map_path.write_text(learning_map)
    print(f"wrote {steps} steps and {concept_count} concepts")


if __name__ == "__main__":
    main()

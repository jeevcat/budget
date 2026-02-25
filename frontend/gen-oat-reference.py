#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["markdownify"]
# ///
"""Scrape https://oat.ink docs and write oat-reference.md."""

import re
import urllib.request

from markdownify import markdownify

PAGES = [
    "https://oat.ink/usage/",
    "https://oat.ink/customizing/",
    "https://oat.ink/components/",
]


def fetch(url: str) -> str:
    req = urllib.request.Request(url, headers={"User-Agent": "gen-oat-reference/1.0"})
    with urllib.request.urlopen(req) as r:
        return r.read().decode()


def extract_main(html: str) -> str:
    m = re.search(r"<main[^>]*>(.*?)</main>", html, re.DOTALL)
    if m:
        return m.group(1)
    return html


LANG_CLASSES = {"language-html": "html", "language-css": "css", "language-js": "js",
                "language-javascript": "js", "language-bash": "bash", "language-sh": "sh"}


def _code_lang(el: dict) -> str:
    for cls in el.get("class") or []:
        if cls in LANG_CLASSES:
            return LANG_CLASSES[cls]
    return ""


def main() -> None:
    parts = [
        "# Oat CSS Reference\n\n"
        "Scraped from https://oat.ink — regenerate with "
        "`./frontend/gen-oat-reference.py`\n"
    ]

    for url in PAGES:
        print(f"Fetching {url}...", flush=True)
        html = fetch(url)
        content = extract_main(html)
        md = markdownify(content, heading_style="ATX", code_language_callback=_code_lang)
        # Strip anchor markers from headings (e.g. "## # Dialog" -> "## Dialog")
        md = re.sub(r"^(#{1,6}) # ", r"\1 ", md, flags=re.MULTILINE)
        # Collapse runs of blank lines
        md = re.sub(r"\n{3,}", "\n\n", md).strip()
        parts.append(f"\n---\n\n<!-- source: {url} -->\n\n{md}\n")

    output = "\n".join(parts)
    with open("frontend/oat-reference.md", "w") as f:
        f.write(output)
    print(f"Wrote frontend/oat-reference.md ({len(output)} bytes)")


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
#Vmseed
#Licensed under the MIT License. See the LICENSE file in the project root.
#Copyright (c) 2026 Scarlett A. Scott (codescarlett)
#
#Contributors:
#Scarlett A. Scott (codescarlett)

"""Checks the static website in site/ before it is deployed.

For every .html page it checks:
  - every relative link and asset (href, src) points at a file that exists,
    and every #anchor on a page points at an id that exists
  - the page has a <title>, a meta description and, apart from 404.html, a
    canonical link (the SEO basics every page is meant to carry)
  - every page listed in sitemap.xml exists

Links to other websites are not fetched, so the check never fails because some
other site is down.

Usage:
    python3 packaging/check_site.py            check site/
    python3 packaging/check_site.py PATH       check another folder

Uses only the standard library.
"""

import re
import sys
from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import unquote, urlsplit

ROOT = Path(__file__).resolve().parent.parent


class Page(HTMLParser):
    """Collects the links, ids and head tags of one page."""

    def __init__(self) -> None:
        super().__init__()
        self.links: list[str] = []
        self.ids: set[str] = set()
        self.title = ""
        self.in_title = False
        self.description = False
        self.canonical = False

    def handle_starttag(self, tag, attrs):
        values = dict(attrs)
        if values.get("id"):
            self.ids.add(values["id"])
        if tag == "a" and values.get("name"):
            self.ids.add(values["name"])
        for attribute in ("href", "src"):
            if values.get(attribute):
                self.links.append(values[attribute])
        if tag == "title":
            self.in_title = True
        if tag == "meta" and values.get("name") == "description" and values.get("content", "").strip():
            self.description = True
        if tag == "link" and values.get("rel") == "canonical" and values.get("href"):
            self.canonical = True

    def handle_endtag(self, tag):
        if tag == "title":
            self.in_title = False

    def handle_data(self, data):
        if self.in_title:
            self.title += data


def target_of(page: Path, link: str, site: Path) -> tuple[Path, str] | None:
    """The file a relative link points at, and its #anchor. None for links this
    check does not follow (other websites, mailto:, javascript:)."""

    parts = urlsplit(link)
    if parts.scheme or parts.netloc or link.startswith("//"):
        return None
    path = unquote(parts.path)
    if not path:
        return page, parts.fragment
    if path.startswith("/"):
        #Absolute paths break when the site is copied under another domain
        #or subfolder, which is the point of keeping it portable.
        raise ValueError("starts with '/'; use a relative link so the site stays portable")
    target = (page.parent / path).resolve()
    if target.is_dir() or path.endswith("/"):
        target = target / "index.html"
    try:
        target.relative_to(site.resolve())
    except ValueError:
        raise ValueError("points outside the site folder") from None
    return target, parts.fragment


def main() -> int:
    site = Path(sys.argv[1]) if len(sys.argv) > 1 else ROOT / "site"
    pages = sorted(site.rglob("*.html"))
    if not pages:
        print(f"no .html pages under {site}", file=sys.stderr)
        return 1

    parsed: dict[Path, Page] = {}
    for path in pages:
        page = Page()
        page.feed(path.read_text(encoding="utf-8"))
        parsed[path.resolve()] = page

    problems: list[str] = []
    for path in pages:
        page = parsed[path.resolve()]
        name = path.relative_to(site)
        if not page.title.strip():
            problems.append(f"{name}: no <title>")
        if not page.description:
            problems.append(f"{name}: no <meta name=\"description\">")
        if not page.canonical and path.name != "404.html":
            problems.append(f"{name}: no <link rel=\"canonical\">")
        for link in page.links:
            try:
                found = target_of(path, link, site)
            except ValueError as reason:
                problems.append(f"{name}: {link} {reason}")
                continue
            if found is None:
                continue
            target, anchor = found
            if not target.exists():
                problems.append(f"{name}: {link} does not exist")
            elif anchor and target.suffix == ".html":
                other = parsed.get(target.resolve())
                if other is not None and anchor not in other.ids:
                    problems.append(f"{name}: {link} has no id=\"{anchor}\"")

    sitemap = site / "sitemap.xml"
    if sitemap.is_file():
        canonical_root = None
        for url in re.findall(r"<loc>\s*([^<\s]+)\s*</loc>", sitemap.read_text(encoding="utf-8")):
            if canonical_root is None:
                canonical_root = url if url.endswith("/") else url.rsplit("/", 1)[0] + "/"
            relative = url[len(canonical_root):] if url.startswith(canonical_root) else None
            if relative is None:
                problems.append(f"sitemap.xml: {url} is not under {canonical_root}")
                continue
            target = site / unquote(relative)
            if relative == "" or relative.endswith("/"):
                target = target / "index.html"
            if not target.is_file():
                problems.append(f"sitemap.xml: {url} has no page ({target.relative_to(site)})")

    for problem in problems:
        print(f"FAIL  {problem}")
    print(f"checked {len(pages)} page(s): {len(problems)} problem(s)")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())

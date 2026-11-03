"""Create a local, inert form-structure fixture from a private HTML capture.

This is a development aid, not an application-page replay. It deliberately
omits scripts, network references, existing values and form submission. Output
is restricted to the ignored target directory and must not be committed.
"""
from __future__ import annotations

import argparse
import html
from html.parser import HTMLParser
from pathlib import Path

MAX_INPUT = 10 * 1024 * 1024
MAX_CONTROLS = 500
MAX_TEXT = 500
INPUT_TYPES = {"text", "email", "tel", "url", "number", "date", "checkbox", "radio", "file"}


class FormReader(HTMLParser):
    def __init__(self) -> None:
        super().__init__(convert_charrefs=True)
        self.controls: list[dict] = []
        self.labels: dict[str, str] = {}
        self._label: dict | None = None
        self._select: dict | None = None
        self._option: list[str] | None = None
        self._textarea = False
        self._ignored = 0

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        if tag in {"script", "style", "template", "noscript"}:
            self._ignored += 1
            return
        if self._ignored:
            return
        attrs = dict(attrs)
        if tag == "label":
            self._label = {"for": attrs.get("for"), "text": [], "controls": []}
        elif tag in {"input", "select", "textarea"} and len(self.controls) < MAX_CONTROLS:
            kind = (attrs.get("type") or "text").lower() if tag == "input" else tag
            if tag == "input" and kind not in INPUT_TYPES:
                return
            control = {"kind": kind, "id": attrs.get("id") or "", "label": attrs.get("aria-label") or attrs.get("placeholder") or "", "required": "required" in attrs, "options": []}
            self.controls.append(control)
            if self._label is not None:
                self._label["controls"].append(control)
            if tag == "select":
                self._select = control
            elif tag == "textarea":
                self._textarea = True
        elif tag == "option" and self._select is not None:
            self._option = []

    def handle_endtag(self, tag: str) -> None:
        if tag in {"script", "style", "template", "noscript"} and self._ignored:
            self._ignored -= 1
            return
        if self._ignored:
            return
        if tag == "label" and self._label is not None:
            text = " ".join("".join(self._label["text"]).split())[:MAX_TEXT]
            if self._label["for"]:
                self.labels[self._label["for"]] = text
            for control in self._label["controls"]:
                control["label"] = text or control["label"]
            self._label = None
        elif tag == "option" and self._option is not None and self._select is not None:
            if len(self._select["options"]) < 100:
                self._select["options"].append(" ".join("".join(self._option).split())[:MAX_TEXT])
            self._option = None
        elif tag == "select":
            self._select = None
        elif tag == "textarea":
            self._textarea = False

    def handle_data(self, data: str) -> None:
        if self._ignored:
            return
        if self._label is not None and self._select is None and not self._textarea:
            self._label["text"].append(data[:MAX_TEXT])
        if self._option is not None:
            self._option.append(data[:MAX_TEXT])


def make_fixture(source: str) -> str:
    parser = FormReader()
    parser.feed(source)
    rows = []
    for index, item in enumerate(parser.controls):
        label = parser.labels.get(item["id"]) or item["label"] or f"Unlabelled control {index + 1}"
        label = html.escape(label[:MAX_TEXT])
        required = " required" if item["required"] else ""
        kind = item["kind"]
        if kind == "select":
            options = "".join(f"<option>{html.escape(value)}</option>" for value in item["options"])
            field = f'<select id="field-{index}"{required}>{options}</select>'
        elif kind == "textarea":
            field = f'<textarea id="field-{index}"{required}></textarea>'
        else:
            field = f'<input id="field-{index}" type="{kind}"{required}>'
        rows.append(f'<p><label for="field-{index}">{label}</label>{field}</p>')
    return ('<!doctype html><meta charset="utf-8">'
            '<meta http-equiv="Content-Security-Policy" content="default-src \'none\'; form-action \'none\'; base-uri \'none\'">'
            '<title>Inert form fixture</title><h1>Inert form fixture</h1>' + "\n".join(rows))


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("input", type=Path, help="Local HTML file; never sent to a service")
    parser.add_argument("output", type=Path, help="New file below this repository's ignored target directory")
    args = parser.parse_args()
    target = (Path(__file__).resolve().parents[1] / "target").resolve()
    output = args.output.resolve()
    if not output.is_relative_to(target) or output == target:
        parser.error("output must be a new file below target")
    with args.input.open("rb") as source:
        raw = source.read(MAX_INPUT + 1)
    if len(raw) > MAX_INPUT:
        parser.error("input exceeds the 10 MiB limit")
    fixture = make_fixture(raw.decode("utf-8", errors="replace"))
    output.parent.mkdir(parents=True, exist_ok=True)
    with output.open("x", encoding="utf-8") as destination:
        destination.write(fixture)
    print("Inert fixture written locally; active markup, network attributes and entered values were omitted.")


if __name__ == "__main__":
    main()

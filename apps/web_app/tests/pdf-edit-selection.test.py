"""Check actionable PDF passage errors with fictional text and real PDFium."""
import base64
import ctypes
import io
import json
from pathlib import Path
import subprocess
import sys

from pypdf import PdfReader, PdfWriter
from pypdf.generic import DecodedStreamObject, DictionaryObject, NameObject
import pypdfium2 as pdfium
import pypdfium2.raw as raw

src = Path(__file__).resolve().parents[1] / "src"
program = "\n".join((src / name).read_text(encoding="utf-8") for name in (
    "pdf_visual_guard.py", "pdf_stream_edit.py", "pdf_edit.py"))


def fixture(operators):
    writer = PdfWriter()
    page = writer.add_blank_page(width=400, height=300)
    font = DictionaryObject({NameObject("/Type"): NameObject("/Font"),
                             NameObject("/Subtype"): NameObject("/Type1"),
                             NameObject("/BaseFont"): NameObject("/Helvetica")})
    page[NameObject("/Resources")] = DictionaryObject({NameObject("/Font"): DictionaryObject({
        NameObject("/F1"): writer._add_object(font)})})
    content = DecodedStreamObject()
    content.set_data(b"BT /F1 12 Tf 20 200 Td " + operators + b" ET")
    page[NameObject("/Contents")] = writer._add_object(content)
    output = io.BytesIO()
    writer.write(output)
    return base64.b64encode(output.getvalue()).decode("ascii")


def refused(operators, passage, expected):
    payload = json.dumps({"pdf": fixture(operators), "before": passage, "after": "Other"})
    result = subprocess.run([sys.executable, "-c", program], input=payload, text=True,
                            capture_output=True, timeout=30, check=False)
    assert result.returncode != 0, "uncertain edit must be refused"
    assert expected in result.stdout, result.stdout + result.stderr
    assert "review manually" in result.stdout


refused(b"(Alpha Beta) Tj", "Missing", "was not found")
refused(b"(Alpha Alpha) Tj", "Alpha", "occurs more than once")
refused(b"(A) Tj (B) Tj (C) Tj (D) Tj (E) Tj (F) Tj", "ABCDEF", "spans PDF text objects")
payload = json.dumps({"pdf": fixture(b"(Alpha) Tj (Beta) Tj"), "before": "AlphaBeta", "after": "AlphaBet"}).encode()
result = subprocess.run([sys.executable, "-c", program], input=payload,
                        capture_output=True, timeout=30, check=False)
assert result.returncode == 0, result.stdout.decode(errors="replace") + result.stderr.decode(errors="replace")
assert PdfReader(io.BytesIO(result.stdout)).pages[0].extract_text() == "AlphaBet"
styled_source = fixture(b"(PrefixAlpha) Tj 0 0 1 rg (BetaSuffix) Tj")
payload = json.dumps({"pdf": styled_source,
                      "before": "AlphaBeta", "after": "AlphaBet"}).encode()
result = subprocess.run([sys.executable, "-c", program], input=payload,
                        capture_output=True, timeout=30, check=False)
assert result.returncode == 0, result.stdout.decode(errors="replace") + result.stderr.decode(errors="replace")
assert PdfReader(io.BytesIO(result.stdout)).pages[0].extract_text() == "PrefixAlphaBetSuffix"
def text_object_colours(data):
    document = pdfium.PdfDocument(data)
    page = document[0]
    colours = []
    for obj in page.get_objects(filter=[raw.FPDF_PAGEOBJ_TEXT], max_depth=1):
        red, green, blue, alpha = (ctypes.c_uint() for _ in range(4))
        assert raw.FPDFPageObj_GetFillColor(obj, ctypes.byref(red), ctypes.byref(green), ctypes.byref(blue), ctypes.byref(alpha))
        colours.append((red.value, green.value, blue.value, alpha.value))
    page.close()
    document.close()
    return colours

assert text_object_colours(result.stdout) == text_object_colours(base64.b64decode(styled_source))
# The whole object may not map to one PDF text operator even when its selected
# span does. Exercise that fallback through the same final validation path.
fallback_program = program.replace("def edit_pdf_stream(reader, page_index, before, after):",
                                   "def original_edit_pdf_stream(reader, page_index, before, after):", 1)
wrapper = """
def edit_pdf_stream(reader, page_index, before, after):
    if before == 'BetaSuffix':
        return None
    if before == 'Beta':
        print('selected-span stream fallback', file=sys.stderr)
    return original_edit_pdf_stream(reader, page_index, before, after)
"""
fallback_program = fallback_program.replace("# Bounded PDF text-object edit.", wrapper + "\n# Bounded PDF text-object edit.", 1)
fallback = subprocess.run([sys.executable, "-c", fallback_program], input=payload,
                          capture_output=True, timeout=30, check=False)
assert fallback.returncode == 0, fallback.stdout.decode(errors="replace") + fallback.stderr.decode(errors="replace")
assert b"selected-span stream fallback" in fallback.stderr
assert PdfReader(io.BytesIO(fallback.stdout)).pages[0].extract_text() == "PrefixAlphaBetSuffix"
assert text_object_colours(fallback.stdout) == text_object_colours(base64.b64decode(styled_source))
# The shorthand text-show operators still pass full text, glyph and visual checks.
for operator in (b"16 TL (ABCD) '", b'16 TL 2 1 (ABCD) "'):
    payload = json.dumps({"pdf": fixture(operator), "before": "ABCD", "after": "BAD"}).encode()
    shorthand = subprocess.run([sys.executable, "-c", program], input=payload,
                               capture_output=True, timeout=30, check=False)
    assert shorthand.returncode == 0, shorthand.stdout.decode(errors="replace") + shorthand.stderr.decode(errors="replace")
    assert PdfReader(io.BytesIO(shorthand.stdout)).pages[0].extract_text() == "BAD"
# More, narrower glyphs may fit inside the original bounds across objects.
payload = json.dumps({"pdf": fixture(b"(WWW) Tj (MMM) Tj"), "before": "WWWMMM", "after": "WWWiiiii"}).encode()
result = subprocess.run([sys.executable, "-c", program], input=payload,
                        capture_output=True, timeout=30, check=False)
assert result.returncode == 0, result.stdout.decode(errors="replace") + result.stderr.decode(errors="replace")
assert PdfReader(io.BytesIO(result.stdout)).pages[0].extract_text() == "WWWiiiii"
# A single object may expand into an otherwise blank part of the page.
payload = json.dumps({"pdf": fixture(b"(Alpha) Tj"), "before": "Alpha", "after": "AlphaBeta"}).encode()
result = subprocess.run([sys.executable, "-c", program], input=payload,
                        capture_output=True, timeout=30, check=False)
assert result.returncode == 0, result.stdout.decode(errors="replace") + result.stderr.decode(errors="replace")
assert PdfReader(io.BytesIO(result.stdout)).pages[0].extract_text() == "AlphaBeta"
# An inserted word must retain a visible gap, not merely extract as a space.
guard = {}
exec((src / "pdf_visual_guard.py").read_text(encoding="utf-8"), guard)
assert not guard["visible_word_gap"]((0, 0, 5, 10), (6, 0, 11, 10))
assert guard["visible_word_gap"]((0, 0, 5, 10), (8, 0, 13, 10))
assert not guard["visible_glyph_box"]((0, 0, 4, 0.01))
assert guard["visible_glyph_box"]((0, 0, 4, 1))
payload = json.dumps({"pdf": fixture(b"(Alpha) Tj"), "before": "Alpha", "after": "Alpha Beta"}).encode()
result = subprocess.run([sys.executable, "-c", program], input=payload,
                        capture_output=True, timeout=30, check=False)
assert result.returncode == 0, result.stdout.decode(errors="replace") + result.stderr.decode(errors="replace")
# The same expansion cannot overwrite another visible object.
payload = json.dumps({"pdf": fixture(b"(Alpha) Tj 40 0 Td (Nearby) Tj"), "before": "Alpha", "after": "AlphaBetaLong"})
result = subprocess.run([sys.executable, "-c", program], input=payload, text=True,
                        capture_output=True, timeout=30, check=False)
assert result.returncode != 0 and "review manually" in result.stdout
# Expansions that do not fit remain subject to the geometry check.
payload = json.dumps({"pdf": fixture(b"(Alpha) Tj (Beta) Tj"), "before": "AlphaBeta", "after": "AlphaBetaLong"})
result = subprocess.run([sys.executable, "-c", program], input=payload, text=True,
                        capture_output=True, timeout=30, check=False)
assert result.returncode != 0 and "review manually" in result.stdout
print("Fictional PDF selection checks passed.")

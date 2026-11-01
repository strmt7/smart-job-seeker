"""Check actionable PDF passage errors with fictional text and real PDFium."""
import base64
import io
import json
from pathlib import Path
import subprocess
import sys

from pypdf import PdfWriter
from pypdf.generic import DecodedStreamObject, DictionaryObject, NameObject

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
refused(b"(Alpha) Tj (Beta) Tj", "AlphaBeta", "spans PDF text objects")
print("Fictional PDF selection checks passed.")

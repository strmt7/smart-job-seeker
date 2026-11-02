"""Exercise the stream editor with a fictional font and explicit Unicode map."""
import importlib.util
import io
from pathlib import Path

from pypdf import PdfReader, PdfWriter
from pypdf.generic import DecodedStreamObject, DictionaryObject, NameObject

module_path = Path(__file__).resolve().parents[1] / "src" / "pdf_stream_edit.py"
spec = importlib.util.spec_from_file_location("waypoint_pdf_stream_edit", module_path)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)

writer = PdfWriter()
page = writer.add_blank_page(width=400, height=300)
mapping = DecodedStreamObject()
mapping.set_data(b"""/CIDInit /ProcSet findresource begin
12 dict begin begincmap
/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def
/CMapName /Fixture def /CMapType 2 def
1 begincodespacerange <00> <FF> endcodespacerange
5 beginbfchar <41> <0041> <42> <0042> <43> <0043> <44> <0044> <45> <0041> endbfchar
endcmap CMapName currentdict /CMap defineresource pop end end""")
font = DictionaryObject({
    NameObject("/Type"): NameObject("/Font"),
    NameObject("/Subtype"): NameObject("/Type1"),
    NameObject("/BaseFont"): NameObject("/Helvetica"),
    NameObject("/ToUnicode"): writer._add_object(mapping),
})
page[NameObject("/Resources")] = DictionaryObject({NameObject("/Font"): DictionaryObject({NameObject("/F1"): writer._add_object(font)})})
content = DecodedStreamObject()
content.set_data(b"BT /F1 12 Tf 20 200 Td (ABCD) Tj ET")
page[NameObject("/Contents")] = writer._add_object(content)
source = io.BytesIO()
writer.write(source)
reader = PdfReader(io.BytesIO(source.getvalue()))
edited = module.edit_pdf_stream(reader, 0, "ABCD", "BAD")
assert edited is not None, "mapped glyphs should use the stream editor"
assert "BAD" in PdfReader(io.BytesIO(edited)).pages[0].extract_text()
assert module.edit_pdf_stream(reader, 0, "ABCD", "Z") is None, "unmapped glyphs need review"
assert module.edit_pdf_stream(reader, 0, "BCD", "A") is None, "ambiguous glyph without source evidence needs review"
content.set_data(b"BT /F1 12 Tf 20 200 Td (AEBCD) Tj ET")
conflicting = io.BytesIO()
writer.write(conflicting)
reader_conflicting = PdfReader(io.BytesIO(conflicting.getvalue()))
assert module.edit_pdf_stream(reader_conflicting, 0, "AABCD", "AB") is None, "conflicting source glyph codes need review"
# Both shorthand text-show operators must preserve their original operator and
# spacing operands while replacing only the string.
from pypdf.generic import ContentStream
for operator in (b"(ABCD) '", b'2 1 (ABCD) "'):
    content.set_data(b"BT /F1 12 Tf 20 200 Td 16 TL " + operator + b" ET")
    fixture = io.BytesIO()
    writer.write(fixture)
    edited = module.edit_pdf_stream(PdfReader(io.BytesIO(fixture.getvalue())), 0, "ABCD", "BAD")
    assert edited is not None, "shorthand text-show operation should be editable"
    result = PdfReader(io.BytesIO(edited))
    assert "BAD" in result.pages[0].extract_text()
    operations = ContentStream(result.pages[0].get_contents(), result).operations
    operands, op = next((args, op) for args, op in operations if op in (b"'", b'"'))
    assert op == operator[-1:]
    if op == b'"':
        assert [int(operands[0]), int(operands[1])] == [2, 1]
print("Fictional PDF charmap stream checks passed.")

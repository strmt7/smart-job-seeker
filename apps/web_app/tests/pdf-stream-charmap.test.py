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

# A subset font lacking a mapped space width can borrow a repeated gap from
# other text operations using that exact font, without editing the old font.
from pypdf.generic import ArrayObject, NumberObject
writer2 = PdfWriter()
page2 = writer2.add_blank_page(width=400, height=300)
cmap2 = DecodedStreamObject()
cmap2.set_data(b"""/CIDInit /ProcSet findresource begin
12 dict begin begincmap /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def
/CMapName /FixtureSpace def /CMapType 2 def 1 begincodespacerange <00> <FF> endcodespacerange
5 beginbfchar <41> <0041> <42> <0042> <43> <0043> <44> <0044> <A0> <0020> endbfchar
endcmap CMapName currentdict /CMap defineresource pop end end""")
font2 = DictionaryObject({NameObject('/Type'):NameObject('/Font'),NameObject('/Subtype'):NameObject('/Type1'),NameObject('/BaseFont'):NameObject('/Helvetica'),NameObject('/ToUnicode'):writer2._add_object(cmap2),NameObject('/FirstChar'):NumberObject(65),NameObject('/LastChar'):NumberObject(68),NameObject('/Widths'):ArrayObject([NumberObject(600)]*4)})
page2[NameObject('/Resources')] = DictionaryObject({NameObject('/Font'):DictionaryObject({NameObject('/F1'):writer2._add_object(font2)})})
stream2 = DecodedStreamObject()
stream2.set_data(b'BT /F1 12 Tf 20 200 Td (ABCD) Tj 0 -20 Td [(AB) -375 (C)] TJ 0 -20 Td [(BC) -375 (D)] TJ ET')
page2[NameObject('/Contents')] = writer2._add_object(stream2)
source2 = io.BytesIO(); writer2.write(source2)
output2 = module.edit_pdf_stream(PdfReader(io.BytesIO(source2.getvalue())), 0, 'ABCD', 'A BCD')
assert output2 is not None
edited2 = PdfReader(io.BytesIO(output2)); fonts2 = edited2.pages[0]['/Resources']['/Font'].get_object()
assert len(fonts2) == 2
assert len(fonts2['/F1'].get_object()['/Widths'].get_object()) == 4
private_font = fonts2['/WaypointSpace0'].get_object()
assert float(private_font['/Widths'].get_object()[160-int(private_font['/FirstChar'])]) == 375
ops2 = ContentStream(edited2.pages[0].get_contents(), edited2).operations
private_index = next(i for i, (args, op) in enumerate(ops2) if op == b'Tf' and args[0] == '/WaypointSpace0')
assert ops2[private_index+2][1] == b'Tf' and ops2[private_index+2][0][0] == '/F1'
# Without repeated spacing evidence no new font resource is invented.
stream2.set_data(b'BT /F1 12 Tf 20 200 Td (ABCD) Tj 0 -20 Td [(AB) -375 (C)] TJ ET')
single = io.BytesIO(); writer2.write(single)
output_single = module.edit_pdf_stream(PdfReader(io.BytesIO(single.getvalue())), 0, 'ABCD', 'A BCD')
assert output_single is not None
assert len(PdfReader(io.BytesIO(output_single)).pages[0]['/Resources']['/Font'].get_object()) == 1
# Conflicting equally frequent gaps do not establish a width.
stream2.set_data(b'BT /F1 12 Tf 20 200 Td (ABCD) Tj 0 -20 Td [(AB) -375 (C)] TJ 0 -20 Td [(BC) -375 (D)] TJ 0 -20 Td [(AB) -250 (C)] TJ 0 -20 Td [(BC) -250 (D)] TJ ET')
ambiguous = io.BytesIO(); writer2.write(ambiguous)
output_ambiguous = module.edit_pdf_stream(PdfReader(io.BytesIO(ambiguous.getvalue())), 0, 'ABCD', 'A BCD')
assert output_ambiguous is not None
assert len(PdfReader(io.BytesIO(output_ambiguous)).pages[0]['/Resources']['/Font'].get_object()) == 1
# Repeated copies of the same textual boundary do not provide unique evidence.
stream2.set_data(b'BT /F1 12 Tf 20 200 Td (ABCD) Tj 0 -20 Td [(AB) -375 (C)] TJ 0 -20 Td [(AB) -375 (C)] TJ ET')
repeated = io.BytesIO(); writer2.write(repeated)
output_repeated = module.edit_pdf_stream(PdfReader(io.BytesIO(repeated.getvalue())), 0, 'ABCD', 'A BCD')
assert output_repeated is not None
assert len(PdfReader(io.BytesIO(output_repeated)).pages[0]['/Resources']['/Font'].get_object()) == 1
print("Fictional PDF charmap stream checks passed.")

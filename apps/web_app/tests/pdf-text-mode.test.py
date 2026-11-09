"""Exercise both extraction modes without candidate or third-party material."""
import io
import json
from pathlib import Path
import subprocess
import sys

from pypdf import PdfWriter
from pypdf.generic import DecodedStreamObject, DictionaryObject, NameObject, NumberObject

writer = PdfWriter()
page = writer.add_blank_page(width=600, height=400)
font = DictionaryObject({NameObject('/Type'): NameObject('/Font'),
                         NameObject('/Subtype'): NameObject('/Type1'),
                         NameObject('/BaseFont'): NameObject('/Helvetica')})
image = DecodedStreamObject()
image.set_data(b'\xff\xff\xff')
image.update({NameObject('/Type'): NameObject('/XObject'),
              NameObject('/Subtype'): NameObject('/Image'),
              NameObject('/Width'): NumberObject(1),
              NameObject('/Height'): NumberObject(1),
              NameObject('/ColorSpace'): NameObject('/DeviceRGB'),
              NameObject('/BitsPerComponent'): NumberObject(8)})
page[NameObject('/Resources')] = DictionaryObject({
    NameObject('/Font'): DictionaryObject({NameObject('/F1'): writer._add_object(font)}),
    NameObject('/XObject'): DictionaryObject({NameObject('/Im1'): writer._add_object(image)}),
})
stream = DecodedStreamObject()
stream.set_data(b'BT /F1 12 Tf 30 350 Td (Fictional skills) Tj '
                b'300 0 Td (Sample experience) Tj ET q 1 0 0 1 20 20 cm /Im1 Do Q')
page[NameObject('/Contents')] = writer._add_object(stream)
writer.add_blank_page(width=600, height=400)
payload = io.BytesIO()
writer.write(payload)
worker = Path(__file__).resolve().parents[1] / 'src' / 'pdf_extract.py'

def run(*args):
    return subprocess.run([sys.executable, '-B', str(worker), *args],
                          input=payload.getvalue(), capture_output=True,
                          timeout=30, check=False)

for args in [(), ('layout',)]:
    result = run(*args)
    assert result.returncode == 0, result.stdout
    output = json.loads(result.stdout)
    for phrase in ['Fictional skills', 'Sample experience']:
        assert phrase in output['text'], (args, output)
    assert output['page_count'] == 2
    assert output['empty_text_pages'] == [2]
    assert output['structurally_blank_pages'] == [2]
    assert output['mixed_image_pages'] == [1]
    assert output['image_inventory_unknown_pages'] == []

for args in [('normal',), ('unknown',), ('layout', 'allow-empty')]:
    result = run(*args)
    assert result.returncode != 0 and b'Invalid PDF extraction mode' in result.stdout
print('Fictional PDF text-mode and coverage checks passed.')

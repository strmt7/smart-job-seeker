"""Exercise the PDF worker page boundary with bounded fictional documents."""
import base64
import io
import json
from pathlib import Path
import subprocess
import sys

from pypdf import PdfWriter

src = Path(__file__).resolve().parents[1] / 'src'
extract = (src / 'pdf_extract.py').read_text(encoding='utf-8')
render = '\n'.join((src / name).read_text(encoding='utf-8') for name in
                   ('pdf_text_geometry.py', 'pdf_render.py'))
edit = '\n'.join((src / name).read_text(encoding='utf-8') for name in
                 ('pdf_visual_guard.py', 'pdf_stream_edit.py', 'pdf_edit.py'))

def document(count):
    writer = PdfWriter()
    for _ in range(count):
        writer.add_blank_page(width=100, height=100)
    output = io.BytesIO()
    writer.write(output)
    return output.getvalue()

def run(program, payload, *args):
    return subprocess.run([sys.executable, '-c', program, *args], input=payload,
                          capture_output=True, timeout=30, check=False,
                          creationflags=subprocess.CREATE_NO_WINDOW if sys.platform == 'win32' else 0)

accepted, rejected = document(50), document(51)
result = run(extract, accepted, 'allow-empty')
assert result.returncode == 0, result.stdout
assert json.loads(result.stdout)['page_count'] == 50
result = run(render, accepted, '49:geometry')
assert result.returncode == 0, result.stdout
assert json.loads(result.stdout)['page'] == 49
assert json.loads(result.stdout)['text_geometry']['status'] == 'no_selectable_text'
for program, payload, args in (
    (extract, rejected, ('allow-empty',)),
    (render, rejected, ('0',)),
    (edit, json.dumps({'pdf':base64.b64encode(rejected).decode(), 'before':'A', 'after':'B'}).encode(), ()),
):
    result = run(program, payload, *args)
    assert result.returncode != 0 and b'50' in result.stdout, result.stdout
print('Fictional PDF page-limit checks passed.')

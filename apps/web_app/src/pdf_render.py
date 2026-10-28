# Render only the requested page in a bounded local worker.
import base64
import io
import json
import math
import sys

try:
    import pypdfium2 as pdfium
    import pypdf.filters as filters
    for name in ("JBIG2_MAX_OUTPUT_LENGTH", "LZW_MAX_OUTPUT_LENGTH", "MAX_ARRAY_BASED_STREAM_OUTPUT_LENGTH", "MAX_DECLARED_STREAM_LENGTH", "RUN_LENGTH_MAX_OUTPUT_LENGTH", "ZLIB_MAX_OUTPUT_LENGTH"):
        if hasattr(filters, name):
            setattr(filters, name, 8 * 1024 * 1024)
    from PIL import Image  # noqa: F401 - required by PdfBitmap.to_pil
    from pypdf import PdfReader

    data = sys.stdin.buffer.read(10 * 1024 * 1024 + 1)
    if len(data) > 10 * 1024 * 1024 or not data.startswith(b'%PDF-'):
        raise ValueError('Invalid or oversized PDF')
    reader = PdfReader(io.BytesIO(data), strict=True)
    if reader.is_encrypted:
        raise ValueError('Encrypted PDFs need manual review')
    index = int(sys.argv[1])
    document = pdfium.PdfDocument(data)
    count = len(document)
    if count < 1 or count > 30 or index < 0 or index >= count:
        raise ValueError('Page is unavailable or document exceeds 30 pages')
    page = document[index]
    width, height = page.get_size()
    if not all(math.isfinite(v) and 1 <= v <= 20000 for v in (width, height)):
        raise ValueError('Unsupported PDF page dimensions')
    scale = min(1.5, 1600 / width, 2200 / height, math.sqrt(3200000 / (width * height)))
    bitmap = page.render(scale=scale)
    image = bitmap.to_pil()
    out = io.BytesIO()
    image.save(out, format='PNG', optimize=False)
    png = out.getvalue()
    if len(png) > 3 * 1024 * 1024:
        raise ValueError('Rendered page exceeds 3 MiB; review the PDF externally')
    print(json.dumps({'page': index, 'pages': count, 'image': base64.b64encode(png).decode('ascii')}))
    image.close()
    bitmap.close()
    page.close()
    document.close()
except ImportError:
    print('PDF preview requires pypdfium2, Pillow and pypdf in the configured local Python runtime')
    sys.exit(2)
except Exception as error:
    print('PDF preview failed: ' + str(error)[:500])
    sys.exit(1)

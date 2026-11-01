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
    page_arg, _, mode = sys.argv[1].partition(':')
    index = int(page_arg)
    if mode not in ('', 'geometry', 'top', 'bottom', 'left', 'right'):
        raise ValueError('Unsupported page region')
    document = pdfium.PdfDocument(data)
    count = len(document)
    if count < 1 or count > 30 or index < 0 or index >= count:
        raise ValueError('Page is unavailable or document exceeds 30 pages')
    page = document[index]
    width, height = page.get_size()
    if not all(math.isfinite(v) and 1 <= v <= 20000 for v in (width, height)):
        raise ValueError('Unsupported PDF page dimensions')
    if mode in ('top', 'bottom', 'left', 'right'):
        # Render the crop directly, preserving the same bounded bitmap budget
        # while giving the selected region more source pixels when possible.
        region_width = width * (0.55 if mode in ('left', 'right') else 1)
        region_height = height * (0.55 if mode in ('top', 'bottom') else 1)
        scale = min(2.25, 1598 / region_width, 2198 / region_height,
                    math.sqrt(3100000 / (region_width * region_height)))
        crops = {'top': (0, height * .45, 0, 0),
                 'bottom': (0, 0, 0, height * .45),
                 'left': (0, 0, width * .45, 0),
                 'right': (width * .45, 0, 0, 0)}
        crop = crops[mode]
        bitmap = page.render(scale=scale, crop=crop)
    else:
        scale = min(1.5, 1600 / width, 2200 / height, math.sqrt(3200000 / (width * height)))
        bitmap = page.render(scale=scale)
    image = bitmap.to_pil()
    out = io.BytesIO()
    image.save(out, format='PNG', optimize=False)
    png = out.getvalue()
    if len(png) > 3 * 1024 * 1024:
        raise ValueError('Rendered page exceeds 3 MiB; review the PDF externally')
    result = {'page': index, 'pages': count, 'region': mode if mode in ('top', 'bottom', 'left', 'right') else 'full', 'scale': scale, 'image': base64.b64encode(png).decode('ascii')}
    if mode == 'geometry':
        # This is an advisory check of selectable text geometry, not an image or
        # semantic assessment. Limit work even on PDFs with pathological text.
        geometry = {'status': 'unavailable', 'overlapping_pairs': 0}
        textpage = None
        try:
            textpage = page.get_textpage()
            count_chars = textpage.count_chars()
            if count_chars == 0:
                geometry['status'] = 'no_selectable_text'
            elif count_chars <= 5000:
                boxes = []
                for i in range(count_chars):
                    left, bottom, right, top = textpage.get_charbox(i)
                    if not all(math.isfinite(v) for v in (left, bottom, right, top)):
                        raise ValueError('Invalid text geometry')
                    if right > left and top > bottom:
                        boxes.append((left, right, bottom, top, i))
                boxes.sort()
                if not boxes:
                    geometry['status'] = 'no_selectable_text'
                active = []
                comparisons = 0
                overlaps = 0
                for left, right, bottom, top, i in boxes:
                    active = [b for b in active if b[1] > left]
                    for other in active:
                        comparisons += 1
                        if comparisons > 100000:
                            break
                        l2, r2, b2, t2, j = other
                        if abs(i - j) <= 1:
                            continue
                        intersection = max(0, min(right, r2) - left) * max(0, min(top, t2) - max(bottom, b2))
                        smaller = min((right-left)*(top-bottom), (r2-l2)*(t2-b2))
                        if intersection >= 0.35 * smaller:
                            overlaps += 1
                    if comparisons > 100000:
                        break
                    active.append((left, right, bottom, top, i))
                if boxes and comparisons <= 100000:
                    geometry = {'status': 'checked', 'overlapping_pairs': overlaps}
        except Exception:
            pass  # An unavailable geometric observation is never a clean result.
        finally:
            if textpage is not None:
                textpage.close()
        result['text_geometry'] = geometry
    print(json.dumps(result))
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

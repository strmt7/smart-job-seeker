# Bounded PDF text-object edit. Preserve original and refuse uncertain layout.
import base64
import ctypes
import io
import json
import math
import sys

try:
    import pypdfium2 as pdfium
    import pypdfium2.raw as raw
    import pypdf.filters as filters
    from pypdf import PdfReader
    for name in ("JBIG2_MAX_OUTPUT_LENGTH", "LZW_MAX_OUTPUT_LENGTH", "MAX_ARRAY_BASED_STREAM_OUTPUT_LENGTH", "MAX_DECLARED_STREAM_LENGTH", "RUN_LENGTH_MAX_OUTPUT_LENGTH", "ZLIB_MAX_OUTPUT_LENGTH"):
        if hasattr(filters, name):
            setattr(filters, name, 8 * 1024 * 1024)
    payload = json.loads(sys.stdin.buffer.read(15 * 1024 * 1024 + 1))
    source = base64.b64decode(payload['pdf'], validate=True)
    before, after = payload['before'], payload['after']
    if len(source) > 10 * 1024 * 1024 or not source.startswith(b'%PDF-'):
        raise ValueError('Invalid or oversized PDF')
    if not before or len(before.encode()) > 1000 or len(after.encode()) > 4000 or any(ord(c) < 32 or ord(c) == 127 for c in before + after):
        raise ValueError('Choose a nonempty single-line passage and replacement')
    reader = PdfReader(io.BytesIO(source), strict=True)
    root = reader.trailer['/Root']
    if reader.is_encrypted or '/AcroForm' in root or '/Perms' in root or b'/ByteRange' in source:
        raise ValueError('Encrypted, signed or interactive PDFs need manual editing')
    doc = pdfium.PdfDocument(source)
    if not 1 <= len(doc) <= 30:
        raise ValueError('PDF exceeds the 30-page limit')
    def matches(text, needle):
        first = text.find(needle)
        if first < 0:
            return 0
        return 2 if text.find(needle, first + 1) >= 0 else 1
    originals, targets = [], []
    for index in range(len(doc)):
        page = doc[index]
        textpage = page.get_textpage()
        text = textpage.get_text_range()
        originals.append(text)
        if sum(len(x) for x in originals) > 200000:
            raise ValueError('PDF text exceeds the review limit')
        for obj in page.get_objects(filter=[raw.FPDF_PAGEOBJ_TEXT], max_depth=1, textpage=textpage):
            value = obj.extract()
            count = matches(value, before)
            if count:
                targets.append((index, value, obj.get_bounds(), count))
        textpage.close()
        page.close()
    if len(targets) != 1 or targets[0][3] != 1 or sum(matches(text, before) for text in originals) != 1:
        raise ValueError('Select a unique passage contained in one PDF text object; review manually')
    index, old_object, old_bounds, _ = targets[0]
    expected_object = old_object.replace(before, after, 1)
    try:
        edited = edit_pdf_stream(reader, index, before, after)
    except (AttributeError, KeyError, TypeError, ValueError, NotImplementedError):
        # Unknown stream/font structures fall back to the original guarded path.
        edited = None
    if edited is None:
        page = doc[index]
        textpage = page.get_textpage()
        objects = [o for o in page.get_objects(filter=[raw.FPDF_PAGEOBJ_TEXT], max_depth=1, textpage=textpage) if o.extract() == old_object]
        if len(objects) != 1:
            raise ValueError('PDF text object is ambiguous; review manually')
        encoded = expected_object.encode('utf-16-le') + b'\0\0'
        chars = (ctypes.c_ushort * (len(encoded) // 2)).from_buffer_copy(encoded)
        if not raw.FPDFText_SetText(objects[0], chars):
            raise ValueError('The original font cannot encode this edit; review manually')
        textpage.close()
        page.gen_content()
        out = io.BytesIO()
        doc.save(out)
        page.close()
        doc.close()
        edited = out.getvalue()
    else:
        doc.close()
    if len(edited) > 10 * 1024 * 1024:
        raise ValueError('Edited PDF exceeds 10 MiB')
    check = pdfium.PdfDocument(edited)
    if len(check) != len(originals):
        raise ValueError('PDF page count changed; review manually')
    original_document = pdfium.PdfDocument(source)
    for page_index in range(len(check)):
        p = check[page_index]
        original_page = original_document[page_index]
        width, height = original_page.get_size()
        if not all(math.isfinite(v) and 1 <= v <= 20000 for v in (width, height)):
            raise ValueError('Unsupported page dimensions; review manually')
        if p.get_size() != original_page.get_size() or p.get_rotation() != original_page.get_rotation():
            raise ValueError('Page geometry changed; review manually')
        scale = min(1.5, 1200 / width, 1600 / height, math.sqrt(1400000 / (width * height)))
        before_bitmap = original_page.render(scale=scale)
        after_bitmap = p.render(scale=scale)
        before_image = before_bitmap.to_pil()
        after_image = after_bitmap.to_pil()
        rectangle = None
        if page_index == index:
            if original_page.get_rotation() != 0:
                raise ValueError('Rotated PDF pages need manual editing')
            page_left, page_bottom, page_right, page_top = original_page.get_bbox()
            if abs((page_right - page_left) - width) > 1 or abs((page_top - page_bottom) - height) > 1:
                raise ValueError('Page crop geometry needs manual editing')
            left, bottom, right, top = old_bounds
            if left < page_left or right > page_right or bottom < page_bottom or top > page_top:
                raise ValueError('Edited text lies outside the page; review manually')
            # Mask the original text object with a small antialiasing margin.
            # Anything else that changed visually causes a manual handoff.
            rectangle = (math.floor((left-page_left)*scale)-4, math.floor((page_top-top)*scale)-4,
                         math.ceil((right-page_left)*scale)+4, math.ceil((page_top-bottom)*scale)+4)
        visual_change, change_bounds = changed_outside(before_image, after_image, rectangle)
        before_image.close()
        after_image.close()
        before_bitmap.close()
        after_bitmap.close()
        if visual_change:
            raise ValueError(f'PDF page {page_index + 1} appearance changed outside the edited text area (render bounds {change_bounds}); review manually')
        original_page.close()
        t = p.get_textpage()
        actual = t.get_text_range()
        expected = originals[page_index].replace(before, after, 1) if page_index == index else originals[page_index]
        if actual != expected:
            raise ValueError('Saved PDF text changed unexpectedly; review manually')
        if page_index == index:
            objs = [o for o in p.get_objects(filter=[raw.FPDF_PAGEOBJ_TEXT], max_depth=1, textpage=t) if o.extract() == expected_object]
            if len(objs) != 1:
                raise ValueError('Edited text object could not be verified')
            obj = objs[0]
            l,b,r,top = obj.get_bounds()
            old_l,old_b,old_r,old_top = old_bounds
            if l < old_l - 1 or b < old_b - 1 or r > old_r + 1 or top > old_top + 1:
                raise ValueError('Replacement extends outside the original text area; review manually')
            obj_addr = ctypes.cast(obj.raw, ctypes.c_void_p).value
            observed = []
            for char_index in range(t.count_chars()):
                char_obj = raw.FPDFText_GetTextObject(t, char_index)
                if not char_obj or ctypes.cast(char_obj, ctypes.c_void_p).value != obj_addr:
                    continue
                code = raw.FPDFText_GetUnicode(t, char_index)
                char = chr(code) if code else ''
                if not char:
                    raise ValueError('A glyph could not be verified; review manually')
                observed.append(char)
                if char and not char.isspace():
                    left,bottom,right,upper = t.get_charbox(char_index)
                    if right - left < 0.01 or upper - bottom < 0.01 or raw.FPDFText_HasUnicodeMapError(t, char_index):
                        raise ValueError('A glyph may be missing in the original font; review manually')
            if ''.join(observed) != expected_object:
                raise ValueError('Edited glyphs could not be matched to the text object; review manually')
        t.close()
        p.close()
    check.close()
    original_document.close()
    sys.stdout.buffer.write(edited)
except ImportError:
    print('PDF editing requires pypdfium2, pypdf and Pillow in the configured local Python runtime')
    sys.exit(2)
except Exception as error:
    print('PDF editing failed: ' + str(error)[:500])
    sys.exit(1)

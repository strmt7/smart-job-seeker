# Local PDF text worker. Input is stdin; output is UTF-8 stdout.
import io
import json
import sys
try:
    import pypdf.filters as filters
    from pypdf import PdfReader
    for name in ("JBIG2_MAX_OUTPUT_LENGTH", "LZW_MAX_OUTPUT_LENGTH", "MAX_ARRAY_BASED_STREAM_OUTPUT_LENGTH", "MAX_DECLARED_STREAM_LENGTH", "RUN_LENGTH_MAX_OUTPUT_LENGTH", "ZLIB_MAX_OUTPUT_LENGTH"):
        if hasattr(filters, name):
            setattr(filters, name, 8 * 1024 * 1024)
    payload = sys.stdin.buffer.read(10 * 1024 * 1024 + 1)
    if not payload.startswith(b"%PDF-") or len(payload) > 10 * 1024 * 1024:
        raise ValueError("Invalid or oversized PDF")
    reader = PdfReader(io.BytesIO(payload), strict=True)
    if reader.is_encrypted:
        raise ValueError("Encrypted PDFs are not supported")
    if len(reader.pages) > 50:
        raise ValueError("PDF exceeds the 50-page limit")
    def image_resources(page):
        # Inspect dictionaries only; never decode image streams. Forms may nest.
        remaining = [256]
        seen = set()
        def visit(resources):
            if resources is None:
                return False
            resources = resources.get_object()
            objects = resources.get("/XObject")
            if objects is None:
                return False
            for obj in objects.get_object().values():
                remaining[0] -= 1
                if remaining[0] < 0:
                    raise ValueError("PDF image resource inventory exceeds limit")
                key = (obj.idnum, obj.generation) if hasattr(obj, "idnum") else id(obj)
                if key in seen:
                    continue
                seen.add(key)
                obj = obj.get_object()
                if obj.get("/Subtype") == "/Image":
                    return True
                if obj.get("/Subtype") == "/Form" and visit(obj.get("/Resources")):
                    return True
            return False
        return visit(page.get("/Resources"))

    parts = []
    size = 0
    empty_pages = []
    structurally_blank_pages = []
    mixed_image_pages = []
    image_inventory_unknown_pages = []
    for index, page in enumerate(reader.pages, 1):
        inline_image = [False]
        def observe_operator(operator, operands, ctm, text_matrix):
            if operator == b"INLINE IMAGE":
                inline_image[0] = True
        text = page.extract_text(visitor_operand_before=observe_operator) or ""
        if text.strip():
            try:
                if inline_image[0] or image_resources(page):
                    mixed_image_pages.append(index)
            except Exception:
                image_inventory_unknown_pages.append(index)
        if not text.strip():
            empty_pages.append(index)
            if "/Contents" not in page and "/Annots" not in page:
                structurally_blank_pages.append(index)
        size += len(text.encode("utf-8"))
        if size > 200000:
            raise ValueError("PDF text exceeds 200 KB")
        parts.append(text)
    result = "\n".join(parts).strip()
    if not result and sys.argv[1:] != ["allow-empty"]:
        raise ValueError("No selectable text was found; scanned PDFs need OCR")
    response = {"text": result, "page_texts": parts, "page_count": len(reader.pages), "empty_text_pages": empty_pages, "structurally_blank_pages": structurally_blank_pages, "mixed_image_pages": mixed_image_pages, "image_inventory_unknown_pages": image_inventory_unknown_pages}
    sys.stdout.buffer.write(json.dumps(response, ensure_ascii=False).encode("utf-8"))
except ImportError:
    print("PDF extraction requires pypdf in the selected Python runtime. Open Settings > Local tools and setup > Set up PDF tools. If WAYPOINT_PDF_PYTHON is set, correct or remove that override first.")
    sys.exit(2)
except Exception as error:
    print("PDF extraction failed: " + str(error)[:500])
    sys.exit(1)

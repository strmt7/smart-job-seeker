# Local PDF text worker. Input is stdin; output is UTF-8 stdout.
import io
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
    if len(reader.pages) > 30:
        raise ValueError("PDF exceeds the 30-page limit")
    parts = []
    size = 0
    for page in reader.pages:
        text = page.extract_text() or ""
        size += len(text.encode("utf-8"))
        if size > 200000:
            raise ValueError("PDF text exceeds 200 KB")
        parts.append(text)
    result = "\n".join(parts).strip()
    if not result:
        raise ValueError("No selectable text was found; scanned PDFs need OCR")
    sys.stdout.buffer.write(result.encode("utf-8"))
except ImportError:
    print("PDF extraction requires pypdf in the configured local Python runtime")
    sys.exit(2)
except Exception as error:
    print("PDF extraction failed: " + str(error)[:500])
    sys.exit(1)

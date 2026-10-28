# Rewrite one PDF text-show operation while retaining surrounding operators.
# Returns None if the font encoding or structure cannot be mapped unambiguously.
def edit_pdf_stream(reader, page_index, before, after):
    from io import BytesIO
    from pypdf import PdfWriter
    from pypdf._cmap import get_encoding
    from pypdf.generic import ByteStringObject, ContentStream, NameObject, TextStringObject

    writer = PdfWriter()
    writer.clone_document_from_reader(reader)
    page = writer.pages[page_index]
    resources = page.get('/Resources')
    if resources is None:
        return None
    fonts = resources.get_object().get('/Font')
    if fonts is None:
        return None
    fonts = fonts.get_object()
    contents = page.get_contents()
    if contents is None:
        return None
    stream = ContentStream(contents, writer)
    current = None
    stack = []
    matches_found = []

    def decode_byte(value, encoding, mapping):
        if not isinstance(encoding, dict):
            return None
        intermediate = encoding.get(value, chr(value))
        if not isinstance(intermediate, str) or len(intermediate) != 1:
            return None
        output = mapping.get(intermediate, intermediate)
        return output if isinstance(output, str) and len(output) == 1 else None

    for op_index, (operands, op) in enumerate(stream.operations):
        if op == b'q':
            if len(stack) >= 100: return None
            stack.append(current)
        elif op == b'Q':
            if not stack: return None
            current = stack.pop()
        elif op == b'Tf':
            try:
                current = get_encoding(fonts[operands[0]].get_object())
            except Exception:
                current = None
        elif op in (b'Tj', b'TJ') and current is not None:
            encoding, mapping = current
            if not isinstance(encoding, dict):
                continue
            elements = operands[0] if op == b'TJ' else operands
            pieces = []
            valid = True
            for offset, item in enumerate(elements):
                if isinstance(item, (TextStringObject, ByteStringObject)):
                    try: data = item.original_bytes
                    except Exception: valid = False; break
                    decoded = [decode_byte(x, encoding, mapping) for x in data]
                    if any(x is None for x in decoded): valid = False; break
                    pieces.append((offset, data, ''.join(decoded)))
            if not valid or not pieces:
                continue
            combined = ''.join(piece[2] for piece in pieces)
            start = combined.find(before)
            if start < 0:
                continue
            if combined.find(before, start + 1) >= 0:
                return None
            matches_found.append((op_index, current, pieces, start, len(before)))
            if len(matches_found) > 1:
                return None
    if len(matches_found) != 1:
        return None
    op_index, (encoding, mapping), pieces, start, length = matches_found[0]
    reverse = {}
    ambiguous = set()
    for value in range(256):
        char = decode_byte(value, encoding, mapping)
        if char is None:
            continue
        if char in reverse:
            ambiguous.add(char)
        else:
            reverse[char] = value
    if any(char not in reverse or char in ambiguous for char in after):
        return None
    replacement = bytes(reverse[char] for char in after)
    operands, op = stream.operations[op_index]
    elements = operands[0] if op == b'TJ' else operands
    position = 0
    inserted = False
    end = start + length
    for offset, data, decoded in pieces:
        next_position = position + len(decoded)
        if start < next_position and end > position:
            local_start = max(start - position, 0)
            local_end = min(end - position, len(decoded))
            new_bytes = data[:local_start] + (replacement if not inserted else b'') + data[local_end:]
            elements[offset] = ByteStringObject(new_bytes)
            inserted = True
        position = next_position
    if not inserted:
        return None
    page[NameObject('/Contents')] = writer._add_object(stream)
    output = BytesIO()
    writer.write(output)
    return output.getvalue()

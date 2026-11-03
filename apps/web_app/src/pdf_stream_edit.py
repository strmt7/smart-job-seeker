# Rewrite one PDF text-show operation while retaining surrounding operators.
# Returns None if the font encoding or structure cannot be mapped unambiguously.
def edit_pdf_stream(reader, page_index, before, after):
    from io import BytesIO
    from pypdf import PdfWriter
    from pypdf._cmap import get_encoding
    from pypdf.generic import ArrayObject, ByteStringObject, ContentStream, DictionaryObject, FloatObject, NameObject, NumberObject, TextStringObject

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
    spacing_evidence = {}
    spacing_overflow = set()

    def decode_byte(value, encoding, mapping):
        if encoding == 'charmap':
            # For a font without an explicit encoding, only trust characters
            # actually present in its ToUnicode map. The later rendering and
            # glyph checks still decide whether the edited PDF is usable.
            result = mapping.get(chr(value))
            return result if isinstance(result, str) and len(result) == 1 else None
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
                font = fonts[operands[0]].get_object()
                encoding, mapping = get_encoding(font)
                current = (encoding, mapping, operands[0], operands[1], font)
            except Exception:
                current = None
        elif op in (b'Tj', b'TJ', b"'", b'"') and current is not None:
            encoding, mapping, font_name, _, _ = current
            if not isinstance(encoding, dict) and encoding != 'charmap':
                continue
            elements = operands[0] if op == b'TJ' else operands
            if op == b'TJ':
                for i in range(1, len(elements)-1):
                    if (isinstance(elements[i-1], (TextStringObject, ByteStringObject))
                            and isinstance(elements[i+1], (TextStringObject, ByteStringObject))
                            and isinstance(elements[i], (NumberObject, FloatObject))):
                        shift = float(elements[i])
                        if -700 <= shift <= -200:
                            name = str(font_name)
                            try:
                                left = elements[i-1].original_bytes
                                right = elements[i+1].original_bytes
                            except Exception:
                                continue
                            if not left or not right or len(left) > 256 or len(right) > 256:
                                continue
                            left_chars = [decode_byte(byte, encoding, mapping) for byte in left]
                            right_chars = [decode_byte(byte, encoding, mapping) for byte in right]
                            if any(char is None for char in left_chars + right_chars):
                                continue
                            boundary = ''.join(left_chars)[-12:] + ' ' + ''.join(right_chars)[:12]
                            entries = spacing_evidence.setdefault(name, [])
                            if len(entries) < 256:
                                entries.append((shift, boundary))
                            else:
                                spacing_overflow.add(name)
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
    op_index, (encoding, mapping, font_name, font_size, font), pieces, start, length = matches_found[0]
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
    # A font may map more than one byte to the same Unicode character. Reuse
    # the source byte only when the selected passage uses a single encoding
    # for that character; never guess between distinct source glyph codes.
    selected_codes = {}
    position = 0
    end = start + length
    for _, data, decoded in pieces:
        for index, char in enumerate(decoded):
            if start <= position + index < end:
                selected_codes.setdefault(char, set()).add(data[index])
        position += len(decoded)
    for char in ambiguous:
        codes = selected_codes.get(char, set())
        if len(codes) == 1:
            reverse[char] = next(iter(codes))
    if any(char not in reverse or (char in ambiguous and len(selected_codes.get(char, ())) != 1) for char in after):
        return None
    replacement = bytes(reverse[char] for char in after)
    operands, op = stream.operations[op_index]
    elements = operands[0] if op == b'TJ' else operands
    position = 0
    inserted = False
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
    # A subset font can map a space in ToUnicode but omit its advance width.
    # If this same font repeatedly uses one unambiguous word-sized TJ gap,
    # create a private font resource with that observed width for this one
    # text operation. The original resource and its other uses remain intact.
    if ' ' in after and encoding == 'charmap' and font.get('/Subtype') == '/Type1' and font.get('/Widths') is not None:
        try:
            code = reverse[' ']
            first = int(font['/FirstChar'])
            widths = list(font['/Widths'].get_object())
            missing = not (first <= code < first + len(widths)) or float(widths[code-first]) == 0
            counts = {}
            candidates = spacing_evidence.get(str(font_name), [])
            if missing and candidates and str(font_name) not in spacing_overflow:
                source_text = page.extract_text() or ''
                if len(source_text) <= 200000:
                    for shift, boundary in candidates:
                        if source_text.count(boundary) == 1:
                            counts[shift] = counts.get(shift, 0) + 1
            ranked = sorted(((count, shift) for shift, count in counts.items() if count >= 2), reverse=True)
            if missing and len(widths) <= 256 and ranked and (len(ranked) == 1 or ranked[0][0] > ranked[1][0]):
                lower, upper = min(first, code), max(first + len(widths) - 1, code)
                if 0 <= lower <= upper <= 255 and float(font_size) > 0:
                    expanded = [NumberObject(0)] * (upper - lower + 1)
                    expanded[first-lower:first-lower+len(widths)] = widths
                    expanded[code-lower] = FloatObject(-ranked[0][1])
                    copied = DictionaryObject()
                    copied.update(font)
                    copied[NameObject('/FirstChar')] = NumberObject(lower)
                    copied[NameObject('/LastChar')] = NumberObject(upper)
                    copied[NameObject('/Widths')] = ArrayObject(expanded)
                    name = NameObject('/WaypointSpace')
                    for index in range(100):
                        name = NameObject(f'/WaypointSpace{index}')
                        if name not in fonts: break
                    else: return None
                    fonts[name] = writer._add_object(copied)
                    stream.operations.insert(op_index, ([name, font_size], b'Tf'))
                    stream.operations.insert(op_index + 2, ([font_name, font_size], b'Tf'))
        except (KeyError, TypeError, ValueError, OverflowError, AttributeError):
            return None
    page[NameObject('/Contents')] = writer._add_object(stream)
    output = BytesIO()
    writer.write(output)
    return output.getvalue()

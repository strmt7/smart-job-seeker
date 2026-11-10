"""Bounded spatial comparison of selectable PDF glyph boxes, not visual approval."""
import math


def inspect_text_geometry(textpage, page_bounds):
    count = textpage.count_chars()
    result = {'status': 'unavailable', 'characters': count, 'overlapping_pairs': 0}
    if count == 0:
        return {**result, 'status': 'no_selectable_text'}
    if not 0 < count <= 5000:
        return {**result, 'reason': 'character_budget'}
    if (len(page_bounds) != 4 or not all(math.isfinite(v) for v in page_bounds)
            or page_bounds[2] <= page_bounds[0] or page_bounds[3] <= page_bounds[1]):
        return {**result, 'reason': 'invalid_page_bounds'}
    boxes = []
    cells = {}
    comparisons = overlaps = clipped = 0
    for index in range(count):
        left, bottom, right, top = textpage.get_charbox(index)
        if not all(math.isfinite(v) for v in (left, bottom, right, top)):
            return {**result, 'reason': 'invalid_text_geometry'}
        if right <= left or top <= bottom:
            continue
        if (left < page_bounds[0] - .5 or bottom < page_bounds[1] - .5
                or right > page_bounds[2] + .5 or top > page_bounds[3] + .5):
            clipped += 1
        # Any positive-area intersection shares a cell. Deduplicate candidates
        # spanning multiple cells before applying the unchanged overlap rule.
        x0, x1 = math.floor(left / 16), math.floor(right / 16)
        y0, y1 = math.floor(bottom / 16), math.floor(top / 16)
        if (x1 - x0 + 1) * (y1 - y0 + 1) > 64:
            return {**result, 'reason': 'glyph_cell_budget', 'comparisons': comparisons}
        keys = [(x, y) for x in range(x0, x1 + 1) for y in range(y0, y1 + 1)]
        candidates = set()
        for key in keys:
            candidates.update(cells.get(key, ()))
        for previous in sorted(candidates):
            comparisons += 1
            if comparisons > 100000:
                return {**result, 'reason': 'comparison_budget', 'comparisons': comparisons}
            l2, b2, r2, t2, original = boxes[previous]
            if abs(index - original) <= 1:
                continue
            intersection = max(0, min(right, r2) - max(left, l2)) * max(0, min(top, t2) - max(bottom, b2))
            smaller = min((right-left)*(top-bottom), (r2-l2)*(t2-b2))
            if intersection >= .35 * smaller:
                overlaps += 1
        previous = len(boxes)
        boxes.append((left, bottom, right, top, index))
        for key in keys:
            cells.setdefault(key, []).append(previous)
    if not boxes:
        return {**result, 'status': 'no_selectable_text'}
    return {**result, 'status': 'checked', 'overlapping_pairs': overlaps,
            'clipped_chars': clipped, 'comparisons': comparisons}

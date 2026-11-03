"""Compare bounded rendered pages while allowing one reviewed text region."""
def changed_outside(before, after, allowed):
    from PIL import ImageChops, ImageDraw
    if before.size != after.size:
        return True, None
    left = before.convert('RGB')
    right = after.convert('RGB')
    difference = ImageChops.difference(left, right)
    left.close()
    right.close()
    if allowed is not None:
        rectangles = allowed if isinstance(allowed, list) else [allowed]
        draw = ImageDraw.Draw(difference)
        for rectangle in rectangles:
            draw.rectangle(rectangle, fill=(0, 0, 0))
    bounds = difference.getbbox()
    difference.close()
    return bounds is not None, bounds


def blank_right_extension(image, start, end, top, bottom):
    """Accept only a bounded, completely white area in the original rendering."""
    if not (0 <= start < end <= image.width and 0 <= top < bottom <= image.height):
        return False
    crop = image.crop((start, top, end, bottom)).convert('RGB')
    extrema = crop.getextrema()
    crop.close()
    return all(low == 255 and high == 255 for low, high in extrema)


def visible_word_gap(previous, following):
    import math
    height = min(previous[3] - previous[1], following[3] - following[1])
    gap = following[0] - previous[2]
    return (all(math.isfinite(x) for x in (height, gap))
            and height > 0 and gap >= max(0.5, height * 0.2))


def visible_glyph_box(box):
    import math
    left, bottom, right, top = box
    return (all(math.isfinite(value) for value in box)
            and right - left >= 0.2 and top - bottom >= 0.2)

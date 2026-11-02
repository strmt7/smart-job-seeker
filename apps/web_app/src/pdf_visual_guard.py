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

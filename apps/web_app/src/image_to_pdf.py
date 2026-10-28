import io
import sys
import warnings

try:
    from PIL import Image, ImageOps

    Image.MAX_IMAGE_PIXELS = 20_000_000
    warnings.simplefilter("error", Image.DecompressionBombWarning)
    source = io.BytesIO(sys.stdin.buffer.read(10 * 1024 * 1024 + 1))
    image = Image.open(source)
    if image.format not in ("PNG", "JPEG") or getattr(image, "n_frames", 1) != 1:
        raise ValueError("Only single-frame PNG and JPEG images are supported")
    if image.width * image.height > 20_000_000 or min(image.size) < 1:
        raise ValueError("Image dimensions exceed the 20 megapixel limit")
    image = ImageOps.exif_transpose(image)
    if image.mode in ("RGBA", "LA") or "transparency" in image.info:
        rgba = image.convert("RGBA")
        background = Image.new("RGB", rgba.size, "white")
        background.paste(rgba, mask=rgba.getchannel("A"))
        image = background
    else:
        image = image.convert("RGB")
    output = io.BytesIO()
    image.save(output, "PDF", resolution=150.0)
    sys.stdout.buffer.write(output.getvalue())
except Exception as error:
    print("Image conversion failed: " + str(error)[:400])
    sys.exit(1)

"""Spatial lookup must preserve overlap semantics without quadratic normal-page work."""
import importlib.util
from pathlib import Path
import random
import unittest

spec = importlib.util.spec_from_file_location('text_geometry', Path(__file__).parents[1] / 'src/pdf_text_geometry.py')
geometry = importlib.util.module_from_spec(spec)
spec.loader.exec_module(geometry)


class TextPage:
    def __init__(self, boxes): self.boxes = boxes
    def count_chars(self): return len(self.boxes)
    def get_charbox(self, index): return self.boxes[index]


class GeometryTests(unittest.TestCase):
    def test_dense_normal_page_completes_with_original_budgets(self):
        page = TextPage([(x * 7, y * 14, x * 7 + 5, y * 14 + 10) for y in range(60) for x in range(80)])
        result = geometry.inspect_text_geometry(page, (0, 0, 600, 850))
        self.assertEqual(result['status'], 'checked')
        self.assertEqual(result['overlapping_pairs'], 0)
        self.assertEqual(result['clipped_chars'], 0)
        self.assertLess(result['comparisons'], 100000)

    def test_spatial_candidates_preserve_pairwise_overlap_and_clipping(self):
        rng = random.Random(451)
        boxes = [(rng.uniform(-5, 100), rng.uniform(-5, 100), rng.uniform(1, 16), rng.uniform(1, 16)) for _ in range(250)]
        boxes = [(x, y, x + w, y + h) for x, y, w, h in boxes]
        expected = 0
        for i, (a, b, c, d) in enumerate(boxes):
            for e, f, g, h in boxes[:max(0, i - 1)]:
                overlap = max(0, min(c, g) - max(a, e)) * max(0, min(d, h) - max(b, f))
                expected += overlap >= .35 * min((c-a)*(d-b), (g-e)*(h-f))
        result = geometry.inspect_text_geometry(TextPage(boxes), (0, 0, 100, 100))
        self.assertEqual(result['status'], 'checked')
        self.assertEqual(result['overlapping_pairs'], expected)
        self.assertEqual(result['clipped_chars'], sum(a < -.5 or b < -.5 or c > 100.5 or d > 100.5 for a, b, c, d in boxes))

    def test_pathological_work_or_invalid_geometry_remains_unknown(self):
        for boxes, reason in [
            ([(0, 0, 5, 10)] * 5001, 'character_budget'),
            ([(0, 0, 5, 10)] * 5000, 'comparison_budget'),
            ([(0, 0, 1000, 1000)], 'glyph_cell_budget'),
            ([(0, 0, float('nan'), 5)], 'invalid_text_geometry'),
        ]:
            result = geometry.inspect_text_geometry(TextPage(boxes), (0, 0, 600, 850))
            self.assertEqual(result['status'], 'unavailable')
            self.assertEqual(result['reason'], reason)


if __name__ == '__main__': unittest.main()

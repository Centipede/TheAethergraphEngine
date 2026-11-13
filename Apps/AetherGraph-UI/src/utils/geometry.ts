import type { CropEdges, PixelRect } from '../types';

/**
 * Convert inset crop edges into an absolute rect within a scan of the given size.
 *
 * `CropEdges` measures pixels *inwards* from each edge, while overlay geometry
 * (and `PixelRect`) needs plain coordinates. This is the single place that
 * conversion happens -- see `PageCanvas.vue` and `PageStrip.vue`.
 */
export function cropEdgesToRect(crop: CropEdges, width: number, height: number): PixelRect {
    return {
        left: crop.left,
        top: crop.top,
        right: width - crop.right,
        bottom: height - crop.bottom,
    };
}

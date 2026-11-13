export type HintType = 'dropcap' | 'image' | 'callout' | 'garbage';

export interface Hint {
    type: HintType;
    letter?: string;
    area: PixelRect;
}

export interface Page {
    index: number;
    name: string;
    scan: string;
    scan_width: number;
    scan_height: number;
    thumb: string;
    thumb_width: number;
    thumb_height: number;
    batch: number;
    import_order: number;
    crop_edges: CropEdges;
    hints?: Hint[];
}

export interface PageDb {
    pages: Page[];
    next_batch: number;
}

/**
 * Crop margins: scan pixels measured *inwards* from each edge of the scan.
 * These are insets, not coordinates -- `right: 100` means 100px in from the
 * right-hand edge. Use `cropEdgesToRect()` to turn them into a `PixelRect`.
 */
export interface CropEdges {
    left:   number;  // scan pixels in from the left edge
    top:    number;  // scan pixels in from the top edge
    right:  number;  // scan pixels in from the right edge
    bottom: number;  // scan pixels in from the bottom edge
}

/** Absolute scan-pixel coordinates of a rectangle's edges. */
export interface PixelRect {
    left:   number;  // scan-space x of the left edge
    top:    number;  // scan-space y of the top edge
    right:  number;  // scan-space x of the right edge
    bottom: number;  // scan-space y of the bottom edge
}

export type ZoomMode = 'NoZoom' | 'CropRegion' | 'PageBBox' | 'Custom';

export type ZoomRequest =
    | { mode: 'NoZoom' }
    | { mode: 'CropRegion'; margin: number }
    | { mode: 'PageBBox'; margin: number }
    | { mode: 'Custom'; area: PixelRect };


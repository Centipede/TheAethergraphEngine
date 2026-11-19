<template>
  <svg
      class="evidence-icon"
      :class="strengthClass"
      :width="size"
      :height="size"
      viewBox="0 0 16 16"
      :stroke="assigned ? 'var(--sl-color-neutral-0, #fff)' : 'currentColor'"
      :stroke-width="kind === 'verdict' ? 1.6 : 2"
      stroke-linecap="round"
      stroke-linejoin="round"
      fill="none"
      aria-hidden="true"
  >
    <!-- The user's own call, rather than the detector's: the glyph inverts onto
         a solid plate. Nothing else in the set can be mistaken for it. -->
    <rect v-if="assigned" x="-1" y="-1" width="18" height="18" rx="4.5"
          fill="currentColor" stroke="none" />

    <!-- ── VERDICT: what we concluded about one edge of this block ───────
         Grammar: vertical motion through the gap = the text continues,
         a horizontal barrier in the gap = it breaks, both at once = the two
         sides disagree, a cross = the block contradicts itself. -->
    <template v-if="kind === 'verdict'">
      <path d="M2.5 4.5H13.5" />
      <path d="M2.5 11.5H13.5" />

      <!-- continuation arrow, present for 'continue' and for 'conflict' -->
      <template v-if="verdict === 'continue' || verdict === 'conflict'">
        <path d="M8 5.4V10.3" />
        <path d="M6.1 8.4 8 10.4 9.9 8.4" />
      </template>

      <!-- barrier, present for 'break' and for 'conflict' -->
      <path v-if="verdict === 'break' || verdict === 'conflict'"
            d="M4.3 8H11.7" stroke-width="2.8" />

      <!-- the block argues with itself -->
      <template v-if="verdict === 'error'">
        <path d="M5.8 5.8 10.2 10.2" />
        <path d="M10.2 5.8 5.8 10.2" />
      </template>

      <!-- looked, could not decide -->
      <path v-if="verdict === 'undetermined'"
            d="M8 5.4V10.6" stroke-dasharray="1 1.6" />
    </template>

    <!-- ── EVIDENCE: one measurement behind a verdict ────────────────────
         Every icon carries the same three-line paragraph body, so it reads
         as "something about a paragraph". The accent is the measured
         feature, and the preceding/following twins are one drawing rotated,
         not two unrelated shapes. -->
    <template v-else>
      <g :transform="shape.rotate ? 'rotate(180 8 8)' : undefined">
        <path v-for="(d, i) in shape.paths" :key="`p${i}`" :d="d" />
        <path v-for="(d, i) in shape.accentPaths || []" :key="`a${i}`"
              :d="d" :stroke-dasharray="dashed ? '1 1.4' : undefined" />
        <rect v-if="shape.accentRect"
              :x="shape.accentRect[0]" :y="shape.accentRect[1]"
              :width="shape.accentRect[2]" :height="shape.accentRect[3]"
              stroke-width="1.4"
              :fill="accentFilled ? (assigned ? 'var(--sl-color-neutral-0, #fff)' : 'currentColor') : 'none'"
              :stroke-dasharray="dashed ? '1 1.4' : undefined" />
      </g>
    </template>

  </svg>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import type { Evidence } from '../types';

export type EvidenceIconKind =
    | 'verdict'
    | 'x_indent'
    | 'x_dedent'
    | 'y_reverse'
    | 'y_advance'
    | 'preceding_terminal'
    | 'following_terminal'
    | 'hyphenation';

/** What a single edge of a block amounts to, once every input is folded in. */
export type Verdict = 'break' | 'continue' | 'conflict' | 'error' | 'undetermined';

const props = withDefaults(defineProps<{
  kind: EvidenceIconKind;
  /** Required when kind is 'verdict'. */
  verdict?: Verdict;
  /** The evidence whose strength the icon reflects. */
  evidence?: Evidence;
  size?: number;
}>(), {
  size: 14,
});

/** The paragraph body shared by the evidence icons. Three strokes, centred on
 *  the box so that a shape and its 180°-rotated twin land in the same place. */
const BODY = ['M2 3.5H14', 'M2 8H14', 'M2 12.5H14'];
/** Two strokes, used where the accent above needs the room. */
const SHORT_BODY = ['M2 9.5H14', 'M2 14H14'];

interface Shape {
  paths: string[];
  accentPaths?: string[];
  /** [x, y, width, height] — the part that fills in as evidence hardens. */
  accentRect?: [number, number, number, number];
  rotate?: boolean;
}

// Each pair is defined once and rotated 180° for its twin: a first line pushed
// in from the left becomes a short last line on the right, a gap above becomes
// a gap below, a slab above becomes a slab below. Everything is drawn at full
// width with heavy strokes, because these are read at 16px over a scan.
const SHAPES: Record<Exclude<EvidenceIconKind, 'verdict'>, Shape> = {
  // The first line is pushed in from the left. The inset itself is the signal,
  // so there is no caliper to muddy it.
  x_indent: {
    paths: ['M7 3.5H14', 'M2 8H14', 'M2 12.5H14'],
  },
  x_dedent: {
    paths: ['M7 3.5H14', 'M2 8H14', 'M2 12.5H14'],
    rotate: true,
  },
  // The neighbour's nearest line, with the vertical gap measured between.
  y_reverse: {
    paths: SHORT_BODY,
    accentPaths: ['M2 2H14'],
    accentRect: [7, 3.4, 2, 4.4],
  },
  y_advance: {
    paths: SHORT_BODY,
    accentPaths: ['M2 2H14'],
    accentRect: [7, 3.4, 2, 4.4],
    rotate: true,
  },
  // The neighbour is a solid thing — a heading, an image — not a paragraph, so
  // nothing can run across this edge.
  preceding_terminal: {
    paths: SHORT_BODY,
    accentRect: [2, 1.4, 11, 4],
  },
  following_terminal: {
    paths: SHORT_BODY,
    accentRect: [2, 1.4, 11, 4],
    rotate: true,
  },
  // Last line broken mid-word: a hyphen, raised clear of the line it ends.
  hyphenation: {
    paths: ['M2 3.5H14', 'M2 8H14', 'M2 12.5H8'],
    accentRect: [10.2, 10.6, 3.8, 1.8],
  },
};

const shape = computed<Shape>(
    () => SHAPES[props.kind as Exclude<EvidenceIconKind, 'verdict'>] ?? { paths: BODY },
);

const strength = computed<string | null>(() => {
  const ev = props.evidence;
  if (!ev) return null;
  if (typeof ev === 'string') return ev; // 'untested' | 'undetermined' | 'error'
  if ('suggested' in ev) return 'suggested';
  if ('determined' in ev) return 'determined';
  if ('assigned' in ev) return 'assigned';
  return null;
});

// Strength is carried by the shape, not only by a colour step: a suggestion is
// an outline, a determination is filled in, and the user's own call adds a ring.
const accentFilled = computed(
    () => strength.value === 'determined' || strength.value === 'assigned',
);
const dashed = computed(() => strength.value === 'undetermined');
const assigned = computed(() => strength.value === 'assigned');
const strengthClass = computed(() => (strength.value ? `evidence-icon--${strength.value}` : null));
</script>

<style scoped>
.evidence-icon {
  display: block;
  flex: none;
  /* The 'assigned' plate bleeds a little past the viewBox. */
  overflow: visible;
}

/* A suggestion should read as lighter than a determination even before the
   fill is noticed. */
.evidence-icon--suggested {
  opacity: 0.75;
}
</style>

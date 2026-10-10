<template>
  <div class="detection-tools">
    <div class="section-header">Detection Thresholds</div>
    <div class="thresholds-container">
      <div class="threshold-header">
        <div></div>
        <div title="Certainly False">CF</div>
        <div title="Suggested False">SF</div>
        <div title="Suggested True">ST</div>
        <div title="Certainly True">CT</div>
      </div>

      <div class="threshold-row">
        <sl-checkbox size="small" :checked="thresholds.use_x_indent" @sl-change="thresholds.use_x_indent = $event.target.checked; onThresholdChange()">X Indent</sl-checkbox>
        <sl-input class="threshold-cf" type="number" size="small" :value="thresholds.x_indent.certainly_false" :disabled="!thresholds.use_x_indent" @sl-change="thresholds.x_indent.certainly_false = parseInt($event.target.value); onThresholdChange()" />
        <sl-input class="threshold-sf" type="number" size="small" :value="thresholds.x_indent.suggested_false" :disabled="!thresholds.use_x_indent" @sl-change="thresholds.x_indent.suggested_false = parseInt($event.target.value); onThresholdChange()" />
        <sl-input class="threshold-st" type="number" size="small" :value="thresholds.x_indent.suggested_true" :disabled="!thresholds.use_x_indent" @sl-change="thresholds.x_indent.suggested_true = parseInt($event.target.value); onThresholdChange()" />
        <sl-input class="threshold-ct" type="number" size="small" :value="thresholds.x_indent.certainly_true" :disabled="!thresholds.use_x_indent" @sl-change="thresholds.x_indent.certainly_true = parseInt($event.target.value); onThresholdChange()" />
      </div>

      <div class="threshold-row">
        <sl-checkbox size="small" :checked="thresholds.use_x_dedent" @sl-change="thresholds.use_x_dedent = $event.target.checked; onThresholdChange()">X Dedent</sl-checkbox>
        <sl-input class="threshold-cf" type="number" size="small" :value="thresholds.x_dedent.certainly_false" :disabled="!thresholds.use_x_dedent" @sl-change="thresholds.x_dedent.certainly_false = parseInt($event.target.value); onThresholdChange()" />
        <sl-input class="threshold-sf" type="number" size="small" :value="thresholds.x_dedent.suggested_false" :disabled="!thresholds.use_x_dedent" @sl-change="thresholds.x_dedent.suggested_false = parseInt($event.target.value); onThresholdChange()" />
        <sl-input class="threshold-st" type="number" size="small" :value="thresholds.x_dedent.suggested_true" :disabled="!thresholds.use_x_dedent" @sl-change="thresholds.x_dedent.suggested_true = parseInt($event.target.value); onThresholdChange()" />
        <sl-input class="threshold-ct" type="number" size="small" :value="thresholds.x_dedent.certainly_true" :disabled="!thresholds.use_x_dedent" @sl-change="thresholds.x_dedent.certainly_true = parseInt($event.target.value); onThresholdChange()" />
      </div>

      <div class="threshold-row">
        <sl-checkbox size="small" :checked="thresholds.use_y_advance" @sl-change="thresholds.use_y_advance = $event.target.checked; onThresholdChange()">Y Advance</sl-checkbox>
        <sl-input class="threshold-cf" type="number" size="small" :value="thresholds.y_advance.certainly_false" :disabled="!thresholds.use_y_advance" @sl-change="thresholds.y_advance.certainly_false = parseInt($event.target.value); onThresholdChange()" />
        <sl-input class="threshold-sf" type="number" size="small" :value="thresholds.y_advance.suggested_false" :disabled="!thresholds.use_y_advance" @sl-change="thresholds.y_advance.suggested_false = parseInt($event.target.value); onThresholdChange()" />
        <sl-input class="threshold-st" type="number" size="small" :value="thresholds.y_advance.suggested_true" :disabled="!thresholds.use_y_advance" @sl-change="thresholds.y_advance.suggested_true = parseInt($event.target.value); onThresholdChange()" />
        <sl-input class="threshold-ct" type="number" size="small" :value="thresholds.y_advance.certainly_true" :disabled="!thresholds.use_y_advance" @sl-change="thresholds.y_advance.certainly_true = parseInt($event.target.value); onThresholdChange()" />
      </div>

      <div class="threshold-row threshold-row--single">
        <sl-checkbox size="small" :checked="thresholds.use_hyphenation" @sl-change="thresholds.use_hyphenation = $event.target.checked; onThresholdChange()">Hyphen</sl-checkbox>
      </div>
    </div>

    <div class="action-buttons">
      <sl-button variant="primary" size="small" @click="onAutoBridgePage" :loading="hocrContext.loading.value">
        Auto Page
      </sl-button>
      <sl-button size="small" @click="onAutoBridgeBlock" :disabled="!selectedBlockId" :loading="hocrContext.loading.value">
        Auto Block
      </sl-button>
    </div>

    <div class="section-header">Active Flows</div>
    <div class="flow-buttons">
      <sl-button
        v-for="flow in flows"
        :key="flow.name"
        :variant="flowSet.has(flow.name) ? 'primary' : 'default'"
        size="small"
        @click="toggleFlow(flow.name)"
      >
        {{ flow.name }}
      </sl-button>
    </div>

    <div v-if="selectedBlock" class="ocr-info-panel">
      <div class="ocr-info-row">
        <span class="ocr-info-label">ID</span>
        <span class="ocr-info-value ocr-info-id">{{ selectedBlock.id }}</span>
      </div>
      <div class="ocr-info-row">
        <span class="ocr-info-label">x_indent</span>
        <span class="ocr-info-value">{{ selectedBlock.metrics?.x_indent ?? '—' }}</span>
      </div>
      <div class="ocr-info-row">
        <span class="ocr-info-label">x_dedent</span>
        <span class="ocr-info-value">{{ selectedBlock.metrics?.x_dedent ?? '—' }}</span>
      </div>
      <div class="ocr-info-row">
        <span class="ocr-info-label">y_advance</span>
        <span class="ocr-info-value">{{ selectedBlock.metrics?.y_advance ?? '—' }}</span>
      </div>
      <div class="ocr-info-row">
        <span class="ocr-info-label">y_reverse</span>
        <span class="ocr-info-value">{{ selectedBlock.metrics?.y_reverse ?? '—' }}</span>
      </div>
      <div class="ocr-info-row">
        <span class="ocr-info-label">has_hyphen</span>
        <span class="ocr-info-value">{{ selectedBlock.metrics ? (selectedBlock.metrics.has_final_hyphen ? 'Yes' : 'No') : '—' }}</span>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import { useHocrContext } from '../composables/useHocr';
import type { DetectionThresholds, HocrBlock } from '../types/hocr';
import { findItem } from '../types/hocr';
import type { Project } from '../types/project';

const props = defineProps<{
  selectedBlockId: string | null;
  project: Project | null;
  flowSet: Set<string>;
  thresholds: DetectionThresholds;
}>();

const emit = defineEmits<{
  (e: 'update:flowSet', flowSet: Set<string>): void;
  (e: 'threshold-change'): void;
  (e: 'auto-bridge-page'): void;
  (e: 'auto-bridge-block'): void;
}>();

const hocrContext = useHocrContext();

const flows = computed(() => props.project?.flows || []);

function toggleFlow(flowName: string) {
  const newSet = new Set(props.flowSet);
  if (newSet.has(flowName)) {
    newSet.delete(flowName);
  } else {
    newSet.add(flowName);
  }
  emit('update:flowSet', newSet);
}

const selectedBlock = computed(() => {
  if (!props.selectedBlockId || !hocrContext.hocrPage.value) return null;
  const item = findItem(hocrContext.hocrPage.value, props.selectedBlockId);
  if (item && item.level === 'block') return item as HocrBlock;
  return null;
});

function onThresholdChange() {
  emit('threshold-change');
}

async function onAutoBridgePage() {
  emit('auto-bridge-page');
}

async function onAutoBridgeBlock() {
  emit('auto-bridge-block');
}
</script>

<style scoped>
.detection-tools {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}

.section-header {
  padding: 0.25rem 0;
  font-size: 0.7rem;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.05em;
  color: var(--color-text-muted, #6c757d);
}

.thresholds-container {
  padding: 0 0 0.5rem;
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
}

.threshold-row {
  display: grid;
  grid-template-columns: 5.5rem repeat(4, 1fr);
  align-items: center;
  gap: 0.2rem;
  min-height: 1.75rem;
}

.threshold-header {
  display: grid;
  grid-template-columns: 5.5rem repeat(4, 1fr);
  gap: 0.2rem;
  font-size: 0.6rem;
  font-weight: 600;
  text-align: center;
  color: var(--color-text-muted, #6c757d);
  margin-bottom: 0.1rem;
}

.threshold-row--single {
  grid-template-columns: 1fr;
}

.threshold-row sl-checkbox,
.threshold-row sl-tooltip,
.threshold-row sl-input {
  min-width: 0;
}

.threshold-row sl-tooltip,
.threshold-row sl-input {
  display: block;
  width: 100%;
}

.threshold-row sl-checkbox::part(label) {
  font-size: 0.8rem;
  user-select: none;
  white-space: nowrap;
}

.threshold-row sl-checkbox::part(label) {
  font-size: 0.8rem;
  user-select: none;
}

.action-buttons {
  display: flex;
  gap: 0.5rem;
  padding: 0 0;
}

.action-buttons sl-button {
  flex: 1;
}

.flow-buttons {
  display: flex;
  flex-wrap: wrap;
  gap: 0.25rem;
}

.flow-buttons sl-button {
  flex: 1 1 auto;
}

.ocr-info-panel {
  border: 1px solid var(--color-border, #dee2e6);
  border-radius: 0.375rem;
  padding: 0.5rem;
  font-size: 0.8rem;
  display: flex;
  flex-direction: column;
  gap: 0.2rem;
}

.ocr-info-row {
  display: flex;
  align-items: baseline;
  gap: 0.35rem;
  flex-wrap: nowrap;
}

.ocr-info-label {
  font-weight: 600;
  color: var(--color-text-muted, #6c757d);
  font-size: 0.65rem;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  flex: 0 0 4.5rem;
}

.ocr-info-value {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 0.75rem;
}

.ocr-info-id {
  font-family: ui-monospace, monospace;
  color: var(--color-text-dimmed, #a2acb6);
}

sl-input {
  width: 100%;
  min-width: 0;
}

sl-input.threshold-cf::part(base) {
  background-color: var(--sl-color-sky-400);
  border-color: var(--sl-color-sky-500);
}
sl-input.threshold-cf::part(input) {
  color: white;
}

sl-input.threshold-sf::part(base) {
  background-color: var(--sl-color-sky-200);
  border-color: var(--sl-color-sky-300);
}
sl-input.threshold-sf::part(input) {
  color: var(--sl-color-sky-900);
}

sl-input.threshold-st::part(base) {
  background-color: var(--sl-color-orange-200);
  border-color: var(--sl-color-orange-300);
}
sl-input.threshold-st::part(input) {
  color: var(--sl-color-orange-900);
}

sl-input.threshold-ct::part(base) {
  background-color: var(--sl-color-orange-400);
  border-color: var(--sl-color-orange-500);
}
sl-input.threshold-ct::part(input) {
  color: white;
}

sl-input::part(input){
  padding-inline: 0.1rem;
}
</style>

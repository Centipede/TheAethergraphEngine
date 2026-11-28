import {
    computed,
    inject,
    onUnmounted,
    provide,
    ref,
    type ComputedRef,
    type InjectionKey,
    type Ref,
} from 'vue';

/** Where an agent says it is working, as reported by GET /api/agent-focus. */
export interface AgentFocus {
    machine_name: string;
    /** Scan stem, i.e. the `:page` route param. */
    page: string;
    /** A folio mode: assist, crop, hint, recognise, edit or bridge. */
    mode: string;
    /** Bumped each time the agent moves, so we can tell "moved again" from "unchanged". */
    seq: number;
    age_seconds: number;
}

export interface AgentFocusContext {
    focus: Ref<AgentFocus | null>;
    isAgentDriving: ComputedRef<boolean>;
    /** Clear the focus server-side and hand the view back to the user. */
    release: () => Promise<void>;
}

const agentFocusKey: InjectionKey<AgentFocusContext> = Symbol('agentFocus');

const POLL_MS = 1000;

/**
 * Focus older than this is ignored.
 *
 * Without it, an agent that stops mid-task would leave the "agent is driving" indicator
 * lit and the view steered indefinitely.
 */
const STALE_AFTER_SECONDS = 60;

export function provideAgentFocusContext() {
    const focus = ref<AgentFocus | null>(null);

    const fetchFocus = async () => {
        try {
            const res = await fetch('/api/agent-focus');
            if (!res.ok) {
                focus.value = null;
                return;
            }
            const body = (await res.json()) as AgentFocus | null;
            focus.value = body && body.age_seconds <= STALE_AFTER_SECONDS ? body : null;
        } catch {
            // Server unreachable: say nobody is driving rather than leave a stale claim up.
            focus.value = null;
        }
    };

    void fetchFocus();
    const timer = window.setInterval(() => void fetchFocus(), POLL_MS);
    onUnmounted(() => window.clearInterval(timer));

    const isAgentDriving = computed(() => focus.value !== null);

    const release = async () => {
        try {
            await fetch('/api/agent-focus', { method: 'DELETE' });
        } catch (e) {
            console.error('Failed to release agent focus:', e);
        }
        focus.value = null;
    };

    const context: AgentFocusContext = { focus, isAgentDriving, release };
    provide(agentFocusKey, context);
    return context;
}

export function useAgentFocusContext() {
    const context = inject(agentFocusKey);
    if (!context) {
        throw new Error('Agent focus context was not provided');
    }
    return context;
}

// PROTOTYPE (throwaway): where Claude lives before a scan. `?claude=A|B|C` picks the variant, `&model=pick|show|none`
// the model treatment. Without `?claude=A|B|C`, or in a production build, nothing here is active and the app is as on
// main. State is in memory only; nothing reaches Rust.
import { useSyncExternalStore } from 'react';

export type ProtoVariant = 'A' | 'B' | 'C';
export type ProtoModelMode = 'pick' | 'show' | 'none';

export const PROTO_VARIANTS: ProtoVariant[] = ['A', 'B', 'C'];
export const PROTO_LABELS: Record<ProtoVariant, string> = {
  A: 'A — Panel from the start',
  B: 'B — Header button, turn-on card',
  C: 'C — After the scan, but says so',
};
export const PROTO_MODES: ProtoModelMode[] = ['pick', 'show', 'none'];

export const PROTO_MODELS = ['Your default (Opus 5.5)', 'Opus 5.5', 'Sonnet 5.5', 'Haiku 5.5'] as const;
export const PROTO_REASONING = ['Default', 'Low', 'Medium', 'High', 'Max'] as const;

interface ProtoState {
  variant: ProtoVariant | null;
  model: ProtoModelMode;
  pickModel: string;
  reasoning: string;
  /** Bumped by variant C's header pill: the plate row and the ghosted rail flash. */
  hint: number;
  /** Variant B: the turn-on card under the header button is open. */
  card: boolean;
}

function read(): ProtoState {
  const q = new URLSearchParams(window.location.search);
  const v = q.get('claude');
  const m = q.get('model');
  return {
    variant: !import.meta.env.PROD && (v === 'A' || v === 'B' || v === 'C') ? v : null,
    model: m === 'show' || m === 'none' ? m : 'pick',
    pickModel: PROTO_MODELS[0],
    reasoning: 'High',
    hint: 0,
    card: false,
  };
}

let state: ProtoState = typeof window === 'undefined' ? { variant: null, model: 'pick', pickModel: PROTO_MODELS[0], reasoning: 'High', hint: 0, card: false } : read();
const listeners = new Set<() => void>();

function set(patch: Partial<ProtoState>) {
  state = { ...state, ...patch };
  listeners.forEach((l) => l());
}

function writeUrl() {
  const q = new URLSearchParams(window.location.search);
  if (state.variant) q.set('claude', state.variant);
  q.set('model', state.model);
  window.history.replaceState(null, '', `${window.location.pathname}?${q.toString()}`);
}

export const proto = {
  setVariant(v: ProtoVariant) {
    set({ variant: v });
    writeUrl();
  },
  setModelMode(m: ProtoModelMode) {
    set({ model: m });
    writeUrl();
  },
  setPickModel(m: string) {
    set({ pickModel: m });
  },
  setReasoning(r: string) {
    set({ reasoning: r });
  },
  setCard(card: boolean) {
    set({ card });
  },
  flashHint() {
    set({ hint: state.hint + 1 });
  },
};

export function useProto(): ProtoState {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => state,
  );
}

/** The model an answer would come from now, as the answer line says it. */
export function answeredBy(s: Pick<ProtoState, 'model' | 'pickModel' | 'reasoning'>): string {
  const model = s.model === 'show' || s.pickModel === PROTO_MODELS[0] ? 'Opus 5.5' : s.pickModel;
  if (s.model === 'show') return `Answered by ${model}`;
  return `Answered by ${model} · ${s.reasoning === 'Default' ? 'default reasoning' : s.reasoning}`;
}

/** The panel head's line, with the model on it when the prototype shows one. */
export function useProtoHead(line: string): string {
  const s = useProto();
  if (!s.variant || s.model === 'none' || !line.startsWith('On')) return line;
  const model = answeredBy(s).replace('Answered by ', '');
  return line === 'On · through your Claude Code' ? `On · ${model}, through your Claude Code` : `${line} · ${model}`;
}

/** Each answer keeps the model it was made with: what was picked when it was first shown. */
const kept = new Map<string, { pickModel: string; reasoning: string }>();
export function keptAnswer(id: string, s: ProtoState): string {
  let k = kept.get(id);
  if (!k) {
    k = { pickModel: s.pickModel, reasoning: s.reasoning };
    kept.set(id, k);
  }
  return answeredBy({ model: s.model, ...k });
}

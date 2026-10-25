// PROTOTYPE (throwaway): the model and reasoning picker (`model=pick`) and the "Answered by" line (`pick`, `show`).
// Visual only: the choice is kept in memory and never sent anywhere.
import { Cpu } from 'lucide-react';
import { PROTO_MODELS, PROTO_REASONING, keptAnswer, proto, useProto, useProtoHead } from './protoState';
import './proto.css';

/** The compact Model and Reasoning control in the panel's status area. Nothing unless `model=pick`. */
export function ProtoModelPicker() {
  const s = useProto();
  if (!s.variant || s.model !== 'pick') return null;
  return (
    <div className="cl-provider cl-privacy-row proto-model">
      <span className="cl-provider-icon" aria-hidden>
        <Cpu size={14} />
      </span>
      <div className="proto-model-grid">
        <label>
          <b>Model</b>
          <select value={s.pickModel} onChange={(e) => proto.setPickModel(e.target.value)} aria-label="Model">
            {PROTO_MODELS.map((m) => (
              <option key={m}>{m}</option>
            ))}
          </select>
        </label>
        <label>
          <b>Reasoning</b>
          <select value={s.reasoning} onChange={(e) => proto.setReasoning(e.target.value)} aria-label="Reasoning">
            {PROTO_REASONING.map((m) => (
              <option key={m}>{m}</option>
            ))}
          </select>
        </label>
        <span className="proto-model-fine">For new questions. Each answer says which model made it.</span>
      </div>
    </div>
  );
}

/** The panel head's line, with the model on it when the prototype shows one. */
export function ProtoHeadLine({ line }: { line: string }) {
  return <>{useProtoHead(line)}</>;
}

/** "Answered by Opus 5.5 · High" under an answer: the model it was made with. Nothing unless `model=pick|show`. */
export function ProtoAnswered({ id }: { id: string }) {
  const s = useProto();
  if (!s.variant || s.model === 'none') return null;
  return (
    <p className="proto-answered">
      <Cpu size={10} aria-hidden /> {keptAnswer(id, s)}
    </p>
  );
}

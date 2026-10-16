import { useEffect, useImperativeHandle, useRef, type Ref } from 'react';
import { createDotField, type DotField as Field } from './field';
import type { DropKind } from './ripple';
import './dotfield.css';

export type { DropKind };

export interface DotFieldHandle {
  /** Drops a ripple at a point in client (viewport) coordinates. */
  drop(clientX: number, clientY: number, kind?: DropKind): void;
}

interface Props {
  ref?: Ref<DotFieldHandle>;
  /** The element the field stays quiet around: the plate's content. */
  calm?: HTMLElement | null;
}

/** Pointer-downs on these never ripple: controls, and anything marked `data-no-ripple`. */
const NO_RIPPLE = 'button, a, input, select, textarea, label, [role="button"], [data-no-ripple]';

/**
 * The living dot field. It fills its parent (which must be positioned) and ripples where the parent is clicked,
 * except on controls. All animation happens in `field.ts` on one canvas; React only wires it to the page, so a
 * frame never touches React state. It stops when unmounted, follows the reduced-motion setting live, and redraws
 * sharply when the window moves to a screen with a different pixel ratio.
 */
export function DotField({ ref, calm = null }: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const fieldRef = useRef<Field | null>(null);
  const calmRef = useRef<HTMLElement | null>(calm);

  useImperativeHandle(
    ref,
    () => ({
      drop(clientX, clientY, kind = 'tap') {
        const canvas = canvasRef.current;
        if (!canvas || !fieldRef.current) return;
        const box = canvas.getBoundingClientRect();
        fieldRef.current.drop(clientX - box.left, clientY - box.top, kind);
      },
    }),
    [],
  );

  useEffect(() => {
    const canvas = canvasRef.current;
    const host = canvas?.parentElement;
    const ctx = canvas?.getContext('2d');
    if (!canvas || !host || !ctx) return; // no 2D canvas (a test DOM): the field simply isn't drawn

    const motion = matchMedia('(prefers-reduced-motion: reduce)');
    const field = createDotField(canvas, ctx, { reducedMotion: motion.matches });
    fieldRef.current = field;

    const sized = new ResizeObserver(() => {
      field.resize();
      measureCalm(canvas, calmRef.current, field);
    });
    sized.observe(canvas);

    const onPointerDown = (e: PointerEvent) => {
      if (e.button !== 0) return;
      if (e.target instanceof Element && e.target.closest(NO_RIPPLE)) return;
      const box = canvas.getBoundingClientRect();
      field.drop(e.clientX - box.left, e.clientY - box.top, 'tap');
    };
    host.addEventListener('pointerdown', onPointerDown);

    const onMotion = () => field.setReducedMotion(motion.matches);
    motion.addEventListener('change', onMotion);

    return () => {
      field.destroy();
      fieldRef.current = null;
      sized.disconnect();
      host.removeEventListener('pointerdown', onPointerDown);
      motion.removeEventListener('change', onMotion);
    };
  }, []);

  // The plate can appear (or change size, as fonts load) after the field starts.
  useEffect(() => {
    calmRef.current = calm;
    const canvas = canvasRef.current;
    const field = fieldRef.current;
    if (!canvas || !field) return;
    measureCalm(canvas, calm, field);
    if (!calm) return;
    const sized = new ResizeObserver(() => measureCalm(canvas, calm, field));
    sized.observe(calm);
    return () => sized.disconnect();
  }, [calm]);

  return <canvas ref={canvasRef} className="dot-field" aria-hidden />;
}

function measureCalm(canvas: HTMLCanvasElement, calm: HTMLElement | null, field: Field) {
  if (!calm) return field.setCalm(null);
  const c = canvas.getBoundingClientRect();
  const r = calm.getBoundingClientRect();
  field.setCalm({ left: r.left - c.left, top: r.top - c.top, width: r.width, height: r.height });
}

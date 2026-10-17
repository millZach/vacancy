import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { useBridge, type RunReport } from '../bridge';
import { overviewModel } from '../map/model';
import { SummaryMap, SummaryPanel } from '../summary/Summary';
import { fade, flyNumbers, prefersReducedMotion, whenDone } from './motion';
import { Wrapped, type Handoff } from './Wrapped';

/**
 * Where the finish is. slides: the highlights alone. fly: the slides make way while the map summary appears under
 * them and the checkout's numbers fly into its panel. fade: the slides fade away over the summary (Skip to recap,
 * and the hand-off with reduced motion: a cross-fade). rest: the map summary. replay: the slides fading back in.
 */
type Phase = 'slides' | 'fly' | 'fade' | 'rest' | 'replay';

interface Props {
  /** The run, as it was kept: every figure the slides and the summary show comes from it. */
  report: RunReport;
  /** Open on the highlight slides (a run just ended) or on the summary (it was seen, or comes from the history). */
  start: 'slides' | 'rest';
  /** The summary came up for the first time. */
  onSeen?: () => void;
  /** The drive's own summary: scan the drive again. */
  onScanAgain?: () => void;
  /** A run from the history: back to where the user was. */
  onClose?: () => void;
}

/**
 * The end of a run: variant D's highlight slides, handing off to the before/after map summary, from which "Replay
 * highlights" plays them again. Used for a run that just ended and for one reopened from the history; both read
 * only the run's report.
 */
export function Finish({ report, start, onSeen, onScanAgain, onClose }: Props) {
  const { platform } = useBridge();
  const [phase, setPhase] = useState<Phase>(start);
  // The flight hands off: the panel's other sections rise in around the numbers landing.
  const [handed, setHanded] = useState(false);
  const [hot, setHot] = useState<string | null>(null);
  const root = useRef<HTMLDivElement>(null);
  const overlay = useRef<HTMLDivElement>(null);
  const model = useMemo(() => overviewModel(report.map), [report.map]);

  // The numbers fly once the summary is laid out under the slides.
  useLayoutEffect(() => {
    if (phase !== 'fly' || !root.current) return;
    let live = true;
    void flyNumbers(root.current).then(() => live && setPhase('rest'));
    return () => {
      live = false;
    };
  }, [phase]);

  // The slides fade away over the summary, or back in over it.
  useLayoutEffect(() => {
    if (phase !== 'fade' && phase !== 'replay') return;
    const out = phase === 'fade';
    const a = fade(overlay.current, out ? 'out' : 'in', out ? '--fade-out-ms' : '--fade-in-ms');
    let live = true;
    void whenDone(a).then(() => live && setPhase(out ? 'rest' : 'slides'));
    return () => {
      live = false;
    };
  }, [phase]);

  // At rest, focus lands on what was freed, at the top of the summary; the first time, the drive remembers it.
  const seen = useRef(start === 'rest');
  const onSeenNow = useRef(onSeen);
  useLayoutEffect(() => {
    onSeenNow.current = onSeen;
  });
  useEffect(() => {
    if (phase !== 'rest') return;
    if (!seen.current) {
      seen.current = true;
      onSeenNow.current?.();
    }
    root.current?.querySelector<HTMLElement>('[data-fly-target="freed"]')?.focus({ preventScroll: true });
  }, [phase]);

  const handoff = (how: Handoff) => {
    const fly = how === 'fly' && !prefersReducedMotion();
    setHanded(fly);
    setPhase(fly ? 'fly' : 'fade');
  };
  const replay = () => {
    setHanded(false);
    setPhase('replay');
  };

  const slidesUp = phase !== 'rest';
  const summaryUp = phase !== 'slides';
  return (
    <div ref={root} className={`finish is-${phase}${handed ? ' is-handed' : ''}`}>
      {summaryUp && (
        <main className="atlas is-map finish-map">
          <SummaryMap model={model} report={report} hot={hot} onHot={setHot} />
        </main>
      )}
      {summaryUp && (
        <SummaryPanel
          report={report}
          os={platform === 'macos' ? 'macOS' : 'Windows'}
          onHot={setHot}
          onReplay={replay}
          {...(onScanAgain ? { onScanAgain } : {})}
          {...(onClose ? { onClose } : {})}
        />
      )}
      {slidesUp && (
        <Wrapped
          report={report}
          leaving={phase === 'fly' ? 'fly' : phase === 'fade' ? 'fade' : null}
          entering={phase === 'replay'}
          onHandoff={handoff}
          overlayRef={overlay}
        />
      )}
    </div>
  );
}

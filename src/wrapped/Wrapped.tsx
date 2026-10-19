import { ChevronLeft, ChevronRight, Pause, Play } from 'lucide-react';
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode, type Ref } from 'react';
import { useBridge, type RunReport } from '../bridge';
import type { OnUndo } from '../tidy/Undo';
import { Wordmark } from '../design/Wordmark';
import { slidesFor, type SlideId } from './deck';
import { countdown, slideMotion, tokenMs, useReducedMotion, whenDone, type Countdown } from './motion';
import {
  CategoriesSlide,
  CheckoutSlide,
  DiskSlide,
  FreedSlide,
  KeptSlide,
  MovedSlide,
  WinSlide,
} from './slides';
import './wrapped.css';

export type Handoff = 'fly' | 'skip';

interface Props {
  report: RunReport;
  /** The slides are making way for the summary: `fly` sends the checkout's numbers there, `fade` fades them out. */
  leaving: 'fly' | 'fade' | null;
  /** Fading in over the summary (Replay highlights): the first slide waits until it has. */
  entering: boolean;
  /** To the summary: from the checkout, or after its countdown (`fly`); Skip to recap or Escape (`skip`). */
  onHandoff: (how: Handoff) => void;
  /** The overlay, which the finish fades as a whole. */
  overlayRef?: Ref<HTMLDivElement>;
  /** Undoes one folder's Tidy up, from the Moved slide. */
  onUndo?: OnUndo;
}

/** A control the space bar already works: Space presses it rather than pausing the slides. */
function isControl(target: EventTarget | null): boolean {
  return target instanceof Element && target.closest('button, summary, a, input, select, textarea, [role="slider"]') !== null;
}

/**
 * The highlight slides over the whole window: variant D's "Wrapped" finish. One slide at a time, each with a
 * countdown in its dot; the last one hands off to the map summary. Arrows go back and forth, Space pauses, Escape
 * skips to the recap; resting the pointer on a slide holds it. Slides glide in and out, or with reduced motion only
 * fade. Screen readers hear each slide's position and name, and its contents once the slides are paused.
 */
export function Wrapped({ report, leaving, entering, onHandoff, overlayRef, onUndo }: Props) {
  const { platform } = useBridge();
  const reduce = useReducedMotion();
  const slides = useMemo(() => slidesFor(report), [report]);
  const last = slides.length - 1;
  /** The slide asked for: its dot is lit and its countdown runs. */
  const [index, setIndex] = useState(0);
  /** The slide on screen, which follows once the one before has left. */
  const [shown, setShown] = useState(0);
  const [paused, setPaused] = useState(false);
  const [hover, setHover] = useState(false);
  const [reading, setReading] = useState(false);
  const running = !paused && !hover && !reading && !leaving && !entering;

  const deck = useRef<HTMLDivElement>(null);
  const section = useRef<HTMLElement>(null);
  const bar = useRef<HTMLElement>(null);
  const timer = useRef<Countdown | null>(null);
  const target = useRef(0);
  const dir = useRef(1);
  const leavingNow = useRef(false);
  // What the event handlers and the countdown need, always current.
  const latest = useRef({ leaving, last, onHandoff, reduce, shown, running });
  useLayoutEffect(() => {
    latest.current = { leaving, last, onHandoff, reduce, shown, running };
  });

  /** Moves to slide `k`; past the last one, hands off to the map. The slide on screen leaves first. */
  const go = useCallback((k: number) => {
    const now = latest.current;
    if (now.leaving) return;
    if (k > now.last) {
      now.onHandoff('fly');
      return;
    }
    const next = Math.max(0, k);
    if (next === target.current) return;
    setReading(false);
    dir.current = next > target.current ? 1 : -1;
    target.current = next;
    setIndex(next);
    if (leavingNow.current) return; // the slide leaving now makes way for the newest choice
    leavingNow.current = true;
    const out = slideMotion(section.current, 'out', dir.current, now.reduce);
    void whenDone(out).then((done) => {
      leavingNow.current = false;
      if (!done && out) return; // the slides closed meanwhile
      if (target.current === latest.current.shown) {
        // Back to the slide that was leaving: bring it straight back.
        out?.cancel();
        slideMotion(section.current, 'in', dir.current, latest.current.reduce);
      } else setShown(target.current);
    });
  }, []);

  // Each slide arrives, and focus never drops to the page when the slide that held it leaves.
  useLayoutEffect(() => {
    slideMotion(section.current, 'in', dir.current, latest.current.reduce);
    const el = deck.current;
    if (el && !el.contains(document.activeElement)) el.focus({ preventScroll: true });
  }, [shown]);

  // The lit dot's countdown. It runs only while the slides play.
  useEffect(() => {
    const ms = tokenMs(index === last ? '--slide-last-ms' : '--slide-ms');
    const c = countdown(bar.current, ms, () => go(index + 1));
    timer.current = c;
    if (latest.current.running) c.play();
    return () => {
      c.cancel();
      timer.current = null;
    };
  }, [index, last, go]);
  useEffect(() => {
    if (running) timer.current?.play();
    else timer.current?.pause();
  }, [running, index]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (latest.current.leaving || e.defaultPrevented || e.altKey || e.ctrlKey || e.metaKey) return;
      if (e.key === 'ArrowRight') go(target.current + 1);
      else if (e.key === 'ArrowLeft') go(target.current - 1);
      else if (e.key === 'Escape') latest.current.onHandoff('skip');
      else if (e.key === ' ' && !isControl(e.target)) setPaused((p) => !p);
      else return;
      e.preventDefault();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [go]);

  const slide = slides[shown]!;
  const body: Record<SlideId, () => ReactNode> = {
    freed: () => <FreedSlide report={report} />,
    disk: () => <DiskSlide report={report} />,
    win: () => <WinSlide report={report} />,
    categories: () => <CategoriesSlide report={report} />,
    moved: () => <MovedSlide report={report} onUndo={onUndo} />,
    kept: () => <KeptSlide report={report} />,
    checkout: () => <CheckoutSlide report={report} onMap={() => onHandoff('fly')} />,
  };
  const cls = ['wrap', `wrap--${slide.id}`];
  if (leaving) cls.push('is-leaving', `is-${leaving}`);
  if (entering) cls.push('is-entering');
  const drag = platform === 'macos' ? { 'data-tauri-drag-region': 'deep' } : {};

  return (
    <div className={cls.join(' ')} ref={overlayRef} data-no-ripple>
      <div className="wrap-bg" aria-hidden />
      <div className="wrap-haze" aria-hidden />
      <div className={shown === 0 ? 'wrap-top is-hidden' : 'wrap-top'} aria-hidden {...drag}>
        <Wordmark lit />
      </div>
      <div
        ref={deck}
        className="wrap-deck"
        role="region"
        aria-roledescription="carousel"
        aria-label="Highlights of this run"
        tabIndex={-1}
      >
        <div className="wrap-stage" aria-live={running ? 'off' : 'polite'}>
          <section
            key={slide.id}
            ref={section}
            className={`slide slide--${slide.id}`}
            role="group"
            aria-roledescription="slide"
            aria-label={`${shown + 1} of ${slides.length}: ${slide.label}`}
            tabIndex={slide.id === 'kept' || slide.id === 'moved' ? 0 : undefined}
            onPointerEnter={() => setHover(true)}
            onPointerLeave={() => setHover(false)}
            onFocus={() => setReading(true)}
            onBlur={event => { if (!event.currentTarget.contains(event.relatedTarget)) setReading(false); }}
          >
            {body[slide.id]()}
          </section>
        </div>
        <nav className="wrap-nav wrap-fade" aria-label="Highlight slides">
          <button type="button" className="round" onClick={() => go(index - 1)} disabled={index === 0} aria-label="Previous slide">
            <ChevronLeft size={18} aria-hidden />
          </button>
          <div className="dots">
            {slides.map((s, k) => (
              <button
                type="button"
                key={s.id}
                className={k === index ? 'dot is-on' : k < index ? 'dot is-past' : 'dot'}
                onClick={() => go(k)}
                aria-label={`Slide ${k + 1} of ${slides.length}: ${s.label}`}
                aria-current={k === index ? 'step' : undefined}
              >
                {k === index && <i key={index} ref={bar} />}
              </button>
            ))}
          </div>
          <button
            type="button"
            className="round"
            onClick={() => go(index + 1)}
            aria-label={index === last ? 'Show the map summary' : 'Next slide'}
          >
            <ChevronRight size={18} aria-hidden />
          </button>
          <button
            type="button"
            className="round is-ghost"
            onClick={() => setPaused((p) => !p)}
            aria-label={paused ? 'Play the slides' : 'Pause the slides'}
          >
            {paused ? <Play size={15} aria-hidden /> : <Pause size={15} aria-hidden />}
          </button>
        </nav>
        {index < last && (
          <button type="button" className="wrap-skip wrap-fade" onClick={() => onHandoff('skip')}>
            Skip to recap
          </button>
        )}
      </div>
    </div>
  );
}

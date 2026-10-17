import './wordmark.css';

/**
 * The neon "Vacancy" sign. Unlit it is a dim tube; `lit` flickers it on (the run's finish lights it in later
 * tickets).
 */
export function Wordmark({ lit = false }: { lit?: boolean }) {
  return (
    <span className={lit ? 'wordmark is-lit' : 'wordmark'} role="img" aria-label="Vacancy">
      Vacancy
    </span>
  );
}

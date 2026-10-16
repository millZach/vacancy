// PROTOTYPE fake desktop window so variants are judged inside realistic chrome.
import type { ReactNode } from 'react';
import type { OS } from './data';
import type { FrameTheme } from './types';

export function WindowFrame({ os, theme, children }: { os: OS; theme: FrameTheme; children: ReactNode }) {
  const mac = os === 'mac';
  return (
    <div className={`pf-window pf-${os}`} style={{ background: theme.bg, borderColor: theme.border }}>
      <div className="pf-titlebar" style={{ color: theme.fg, borderColor: theme.border }}>
        {mac ? (
          <>
            <span className="pf-lights"><i /><i /><i /></span>
            <span className="pf-title pf-title-center">Vacancy</span>
          </>
        ) : (
          <>
            <span className="pf-title">Vacancy</span>
            <span className="pf-caption"><b>&#x2014;</b><b>&#x25A2;</b><b>&#x2715;</b></span>
          </>
        )}
      </div>
      <div className="pf-content">{children}</div>
    </div>
  );
}

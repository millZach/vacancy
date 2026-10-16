// PROTOTYPE top strip: the motel sign (NO lights up while the disk is nearly full) and the live disk meter.
import { HardDrive } from 'lucide-react';
import { formatBytes, type Dataset } from '../../shared/data';

const FULL = 0.85;

export function Header({ data, freed }: { data: Dataset; freed: number }) {
  const { totalBytes, usedBytes, name } = data.disk;
  const used = usedBytes - freed;
  const frac = used / totalBytes;
  const full = frac > FULL;
  return (
    <header className="vb-top">
      <div className={`vb-sign ${full ? 'vb-sign-full' : 'vb-sign-free'}`} role="img" aria-label={full ? 'No vacancy: disk nearly full' : 'Vacancy: disk has room'}>
        <span className="vb-sign-no">No</span>
        <span className="vb-sign-word">Vacancy</span>
      </div>
      <div className="vb-top-disk">
        <HardDrive size={15} strokeWidth={1.75} aria-hidden />
        <span className="vb-top-name">{name}</span>
        <span className="vb-top-bar" aria-hidden>
          <i style={{ width: `${(frac * 100).toFixed(2)}%` }} className={full ? 'vb-top-bar-full' : ''} />
        </span>
        <span className="vb-top-free">
          <b className={freed > 0 ? 'vb-top-gain' : ''}>{formatBytes(totalBytes - used)}</b> free of {formatBytes(totalBytes, 0)}
        </span>
      </div>
    </header>
  );
}

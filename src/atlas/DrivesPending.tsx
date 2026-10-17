import { HardDrive } from 'lucide-react';
import { DotField } from '../dotfield/DotField';
import '../scan/scan.css';
import './atlas.css';

/** The scan screen before the drives are known: the dot field with an empty plate, or why the drive can't be read. */
export function DrivesPending({ error }: { error: string | null }) {
  return (
    <main className="atlas is-scan">
      <DotField />
      <div className="atlas-toolbar" />
      <div className="atlas-stage">
        <div className="plate">
          <div className="plate-content" aria-busy={error === null}>
            <HardDrive size={26} strokeWidth={1.4} className="plate-icon" aria-hidden />
            {error !== null && (
              <>
                <h1>Can’t read your system drive</h1>
                <p className="plate-cap plate-error" role="alert">
                  {error}
                </p>
              </>
            )}
            <p className="plate-fine">Scanning only reads. Nothing changes until you approve a plan.</p>
          </div>
        </div>
      </div>
    </main>
  );
}

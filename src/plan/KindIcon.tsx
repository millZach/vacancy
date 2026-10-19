import { File, FileArchive, FileText, Film, Image, Music } from 'lucide-react';
import type { FileKind } from '../bridge';

const ICONS = { photo: Image, video: Film, audio: Music, document: FileText, archive: FileArchive, other: File };
const LABELS: Record<FileKind, string> = {
  photo: 'Photo',
  video: 'Video',
  audio: 'Audio file',
  document: 'Document',
  archive: 'Archive',
  other: 'File',
};

/** A file type's icon. Vacancy never shows what is in a file: no thumbnails, only its kind. */
export function KindIcon({ kind, size = 14 }: { kind: FileKind; size?: number }) {
  const Icon = ICONS[kind];
  return <Icon size={size} aria-hidden data-kind={kind} />;
}

/** A copy as a square with its file type's icon, a tag saying what happens to it, and its folder under it. */
export function CopyTile({ kind, tag, folder, gone }: { kind: FileKind; tag: string; folder: string; gone?: boolean }) {
  return (
    <span className="dup-copy">
      <span className={gone ? 'dup-icon is-gone' : 'dup-icon'} title={LABELS[kind]}>
        <KindIcon kind={kind} size={17} />
        <em className="dup-icon-tag">{tag}</em>
      </span>
      <small title={folder}>{folder}</small>
    </span>
  );
}

"""Writes small.expected.tsv from small.manifest.json: what a Windows listing of the image would show for every name
(bigdir's 3,000 one-byte files are checked by pattern instead, so they are left out), from the NTFS attributes
ntfsinfo read, by the rules ADR 0007 gives: a resident stream's allocation is its length rounded up to 8; a
non-resident one's is its allocated size, or its compressed size when compressed or sparse; a Windows Overlay
Filter file's is its WofCompressedData stream's.

    python expect.py small.manifest.json small.expected.tsv

Columns: path, record, kind (dir, file, dirlink, filelink, clouddir), logical, allocated.
"""
import json
import sys

src, out = sys.argv[1], sys.argv[2]
m = json.load(open(src, encoding='utf-8'))
records = m['records']


def data_sizes(inode):
    rec = records.get(str(inode))
    if rec is None:
        return None
    unnamed = [a for a in rec['attributes'] if a['type'] == '$DATA' and not a.get('name')]
    wof = [a for a in rec['attributes'] if a['type'] == '$DATA' and a.get('name') == 'WofCompressedData']
    reparse = [a for a in rec['attributes'] if a['type'] == '$REPARSE_POINT']
    tag = int(reparse[0].get('reparse_tag', '0'), 16) if reparse and isinstance(reparse[0].get('reparse_tag'), str) else None
    if not unnamed:
        return (0, 0, tag)
    a = [x for x in unnamed if x.get('resident') or x.get('lowest_vcn', 0) == 0][0]
    logical = a['data_size']
    if a['resident']:
        alloc = (logical + 7) & ~7
    elif int(a['flags'], 16) & 0x8001:
        alloc = a.get('compressed_size', a['allocated_size'])
    else:
        alloc = a['allocated_size']
    if tag == 0x80000017 and wof:
        alloc = wof[0]['allocated_size']
    return (logical, alloc, tag)


rows = []
for path, e in sorted(m['entries'].items()):
    if path == '' or path.startswith('bigdir/f'):
        continue
    inode = e['inode']
    sizes = data_sizes(inode)
    tag = int(e['reparse_tag'], 16) if 'reparse_tag' in e else (sizes[2] if sizes else None)
    is_dir = e['kind'] == 'dir' or int(e.get('attributes', '0'), 16) & 0x10
    if is_dir:
        if tag is None:
            kind = 'dir'
        elif (tag & 0xFFFF0FFF) == 0x9000001A:
            kind = 'clouddir'
        else:
            kind = 'dirlink'
        logical = alloc = 0
    else:
        kind = 'filelink' if tag is not None and tag & 0x20000000 else 'file'
        logical, alloc = (sizes[0], sizes[1]) if sizes else (e['size'], e['allocated'])
        if kind == 'filelink':
            logical = alloc = 0
    rows.append((path, inode, kind, logical, alloc))

with open(out, 'w', encoding='utf-8', newline='\n') as f:
    f.write('path\trecord\tkind\tlogical\tallocated\n')
    for r in rows:
        f.write('\t'.join(str(x) for x in r) + '\n')
print(len(rows), 'rows')

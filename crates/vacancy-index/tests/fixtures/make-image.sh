#!/bin/sh
# Regenerates small.ntfs.gz and small.expected.tsv: a real NTFS volume
# written by an independent implementation (ntfs-3g), for the MFT parser tests.
#
# How the tools were obtained, without root (WSL2 Ubuntu 26.04, no sudo):
#   mkdir -p ~/ntfs-tools/dl/fuse2 ~/ntfs-tools/dl/src ~/ntfs-tools/build
#   cd ~/ntfs-tools/dl/fuse2 && apt-get download libfuse-dev libfuse2t64
#   for d in *.deb; do dpkg -x "$d" ~/ntfs-tools/fuse2root; done
#   cd ~/ntfs-tools/dl/src && curl -sSLfo rel.tgz \
#       https://tuxera.com/opensource/ntfs-3g_ntfsprogs-2022.10.3.tgz
#   cd ~/ntfs-tools/build && tar xzf ../dl/src/rel.tgz && cd ntfs-3g_ntfsprogs-2022.10.3
#   F=~/ntfs-tools/fuse2root/usr; T=~/ntfs-tools
#   # No pkg-config is installed. configure only needs one to exist; the real
#   # flags come from FUSE_MODULE_*. ~/ntfs-tools/fakebin/pkg-config is:
#   #   case "$*" in *--version*) echo 0.29.2 ;;
#   #     *--libs-only-L*) echo -L$HOME/ntfs-tools/fuse2root/usr/lib/x86_64-linux-gnu ;; esac
#   PATH=$T/fakebin:$PATH \
#   FUSE_MODULE_CFLAGS="-I$F/include/fuse -D_FILE_OFFSET_BITS=64" \
#   FUSE_MODULE_LIBS="-L$F/lib/x86_64-linux-gnu -lfuse -pthread -Wl,-rpath,$F/lib/x86_64-linux-gnu" \
#   ./configure --prefix=$T/inst --exec-prefix=$T/inst --with-fuse=external \
#       --disable-mount-helper --disable-ldconfig --disable-mtab --enable-extras \
#       --disable-crypto LDFLAGS="-Wl,-rpath,$T/inst/lib"
#   make -j8 && make install
# (~/ntfs-tools/build.sh holds exactly these steps.)
# The distro's ntfs-3g uses its internal FUSE, which needs root to mount.
# Built against external libfuse 2 it mounts through the setuid fusermount
# (here a link to fusermount3), so an ordinary user can mount an image file.
# Mounts pass no_def_opts: ntfs-3g's default allow_other is refused by
# fusermount unless /etc/fuse.conf has user_allow_other.
# Needs python3 (3.8+, for os.setxattr) on the Linux side.
#
# Usage (inside WSL):  sh make-image.sh
#   NTFS_TOOLS  tool folder (default ~/ntfs-tools; binaries in $NTFS_TOOLS/inst)
#   WORK        scratch folder on a Linux file system (default $NTFS_TOOLS/work)
# The image differs byte for byte on every run (timestamps, GUIDs, $LogFile),
# so the manifest records the sha256 of the image it describes.
set -eu
export LC_ALL=C.UTF-8   # ntfsinfo and ntfsls print names in the locale's charset

TOOLS=${NTFS_TOOLS:-$HOME/ntfs-tools}
BIN=$TOOLS/inst/bin
SBIN=$TOOLS/inst/sbin
OUT=$(cd "$(dirname "$0")" && pwd)
WORK=${WORK:-$TOOLS/work}
IMG=$WORK/small.img
MNT=$WORK/mnt
SERIAL=0x5641434e5446530a   # fixed volume serial, so it is stable across runs

fusermount -u "$MNT" 2>/dev/null || true
rm -rf "$WORK"
mkdir -p "$MNT"

# 16 MiB is plenty: the MFT for 3,100 records is the biggest thing in it.
truncate -s 16M "$IMG"
"$SBIN/mkntfs" -F -Q -q -c 4096 -s 512 -L vacancy-small "$IMG"
"$SBIN/ntfslabel" --new-serial=${SERIAL#0x} "$IMG" vacancy-small >/dev/null

mount_img() {  # mount_img <extra options>; sets NTFS_PID
    "$BIN/ntfs-3g" -o "no_def_opts,no_detach,$1" "$IMG" "$MNT" &
    NTFS_PID=$!
    i=0
    until grep -qs " $MNT fuseblk\| $MNT fuse" /proc/mounts; do
        i=$((i + 1)); [ $i -lt 100 ] || { echo "mount failed" >&2; exit 1; }
        sleep 0.1
    done
}
umount_img() {
    fusermount -u "$MNT"
    wait "$NTFS_PID"
}

# ---- populate ---------------------------------------------------------------
mount_img "rw,streams_interface=windows,compression"
python3 -I - "$MNT" "$WORK/deleted.inode" <<'PY'
import os, struct, sys
root, deleted_out = sys.argv[1], sys.argv[2]
P = lambda *a: os.path.join(root, *a)

def write(rel, data):
    with open(P(rel), "wb") as f:
        f.write(data)

def utf16(s):
    return s.encode("utf-16-le")

def reparse(tag, payload):
    return struct.pack("<IHH", tag, len(payload), 0) + payload

def mount_point(target, printed):
    sub, prn = utf16(target), utf16(printed)
    # SubstituteName then PrintName, each NUL terminated, as Windows writes them.
    body = struct.pack("<HHHH", 0, len(sub), len(sub) + 2, len(prn))
    return reparse(0xA0000003, body + sub + b"\0\0" + prn + b"\0\0")

def symlink(target, printed):
    sub, prn = utf16(target), utf16(printed)
    body = struct.pack("<HHHHI", 0, len(sub), len(sub), len(prn), 0)  # Flags 0: absolute
    return reparse(0xA000000C, body + sub + prn)

def set_reparse(rel, data):
    os.setxattr(P(rel), "system.ntfs_reparse_data", data)

def pattern(n, seed):
    return bytes(((i * 31 + seed) % 255) + 1 for i in range(n))  # never zero

write("tiny.txt", b"hello")
write("medium.bin", pattern(100_000, 7))
write("with-stream.txt", b"twenty bytes of txt\n")
write("with-stream.txt:extra", pattern(70_000, 11))

with open(P("sparse.bin"), "wb") as f:
    f.truncate(1 << 20)
    f.seek(512 * 1024)
    f.write(pattern(4096, 13))

os.mkdir(P("compressed"))
os.setxattr(P("compressed"), "system.ntfs_attrib_be", struct.pack(">I", 0x00000810))
line = b"Vacancy compressible text. The quick brown fox jumps over the lazy dog.\n"
text = (line * (300_000 // len(line) + 1))[:300_000]
write("compressed/big.txt", text)

write("long-name-" + "0123456789" * 19, b"long name\n")
write("\u00fcn\u00efc\u00f8d\u00e9-\u65e5\u672c\u8a9e-\U0001F600.txt", b"unicode!!\n")

os.mkdir(P("Program Files Long Folder Name"))
write("Program Files Long Folder Name/inner.txt", b"inner txt\n")
os.setxattr(P("Program Files Long Folder Name"), "system.ntfs_dos_name", b"PROGRA~1")

os.mkdir(P("links"))
os.mkdir(P("other"))
write("links/a.bin", pattern(50_000, 17))
os.link(P("links/a.bin"), P("links/b.bin"))
os.link(P("links/a.bin"), P("other/c.bin"))

os.mkdir(P("manylinks"))
write("manylinks/target.bin", pattern(8_000, 19))
for i in range(1, 31):
    head = f"link-{i:02d}-"
    name = head + "L" * (120 - len(head) - 4) + ".bin"
    assert len(name) == 120
    os.link(P("manylinks/target.bin"), P("manylinks", name))

os.mkdir(P("bigdir"))
for i in range(3000):
    write(f"bigdir/f{i:05d}.txt", b"x")

d = P("deep")
os.mkdir(d)
for i in range(1, 41):
    d = os.path.join(d, f"d{i:02d}")
    os.mkdir(d)
with open(os.path.join(d, "leaf.txt"), "wb") as f:
    f.write(b"leaf leaf\n")

os.mkdir(P("junction"))
set_reparse("junction", mount_point("\\??\\C:\\Windows", "C:\\Windows"))

write("symlink.txt", b"")
set_reparse("symlink.txt", symlink("\\??\\C:\\x.txt", "C:\\x.txt"))

os.mkdir(P("cloud"))
with open(P("cloud/placeholder.txt"), "wb") as f:
    f.truncate(1 << 20)
# Cloud files tag; the payload is opaque to anyone but the cloud filter.
set_reparse("cloud", reparse(0x9000001A, b"VACANCY-CLOUD-PLACEHOLDER\0\0\0"))

with open(P("wof.bin"), "wb") as f:
    f.truncate(200_000)
write("wof.bin:WofCompressedData", pattern(30_000, 23))
# WOF_EXTERNAL_INFO {Version 1, Provider 2 = file} + FILE_PROVIDER_EXTERNAL_INFO_V1
# {Version 1, Algorithm 0 = XPRESS4K}, as Windows writes for compact.exe files.
set_reparse("wof.bin", reparse(0x80000017, struct.pack("<IIII", 1, 2, 1, 0)))

write("empty.txt", b"")

write("deleted.bin", pattern(20_000, 29))
with open(deleted_out, "w") as f:
    f.write(str(os.stat(P("deleted.bin")).st_ino))
os.unlink(P("deleted.bin"))
PY
umount_img

# ---- manifest ---------------------------------------------------------------
# Pass 1: read back through a read-only mount. streams_interface=xattr shows
# named data streams as user.<name> extended attributes.
mount_img "ro,streams_interface=xattr"
python3 -I - "$MNT" "$WORK/mounted.json" <<'PY'
import json, os, stat, struct, sys
root, out = sys.argv[1], sys.argv[2]

def xattr(path, name):
    try:
        return os.getxattr(path, name, follow_symlinks=False)
    except OSError:
        return None

entries = {}
unreachable = []

def visit(rel, path):
    st = os.lstat(path)
    attrib = xattr(path, "system.ntfs_attrib")
    attrib = struct.unpack("<I", attrib)[0] if attrib else None
    reparse = xattr(path, "system.ntfs_reparse_data")
    dos = xattr(path, "system.ntfs_dos_name")
    # Names only: a stream over 64 KiB is too big to read as an xattr, so the
    # sizes come from ntfsinfo in pass 2.
    streams = [n[5:] for n in os.listxattr(path, follow_symlinks=False) if n.startswith("user.")]
    is_dir = bool(attrib & 0x10) if attrib is not None else stat.S_ISDIR(st.st_mode)
    e = {
        "inode": st.st_ino,
        "kind": "dir" if is_dir else "file",
        "posix_type": stat.filemode(st.st_mode)[0],
        "size": st.st_size,
        "allocated": st.st_blocks * 512,
        "links": st.st_nlink,
    }
    if reparse:
        e["reparse_tag"] = "0x%08X" % struct.unpack_from("<I", reparse)[0]
        e["reparse_data"] = reparse.hex()
    if attrib is not None:
        e["attributes"] = "0x%08X" % attrib
    if streams:
        e["streams"] = streams
    if dos:
        e["dos_name"] = dos.decode()
    entries[rel] = e
    if is_dir:
        try:
            names = sorted(os.listdir(path))
        except OSError as err:
            # ntfs-3g shows an unknown reparse tag as a symlink it can't follow.
            unreachable.append(rel)
            e["listing_error"] = err.strerror
            return
        for n in names:
            visit(n if rel == "" else rel + "/" + n, os.path.join(path, n))

visit("", root)
with open(out, "w") as f:
    json.dump({"entries": entries, "unreachable": unreachable}, f)
PY
umount_img

# Pass 2: ntfsinfo (ntfsprogs, same release) on the unmounted image for the
# record-level facts, and ntfsls for directories the mount can't enter.
export NTFSINFO="$BIN/ntfsinfo" NTFSLS="$BIN/ntfsls" NTFS3G="$BIN/ntfs-3g" MKNTFS="$SBIN/mkntfs"
python3 -I - "$IMG" "$WORK/mounted.json" "$WORK/deleted.inode" "$WORK/small.manifest.json" <<'PY'
import hashlib, json, os, re, struct, subprocess, sys
img, mounted, deleted_path, out = sys.argv[1:5]
env = os.environ
m = json.load(open(mounted))
entries = m["entries"]
deleted_inode = int(open(deleted_path).read())

def run(*args):
    p = subprocess.run(args, capture_output=True, text=True)
    return p.stdout + p.stderr

def num(v):
    return int(v.split()[0])

def ntfsinfo(inode):
    text = subprocess.run([env["NTFSINFO"], "-v", "-i", str(inode), img],
                          capture_output=True, text=True).stdout
    rec = {"records": [], "names": [], "attributes": []}
    cur = None
    in_runlist = False
    for line in text.splitlines():
        mm = re.match(r"Dumping attribute (\S+) \((0x[0-9a-f]+)\) from mft record (\d+)", line)
        if mm:
            cur = {"type": mm.group(1), "record": int(mm.group(3))}
            rec["attributes"].append(cur)
            if int(mm.group(3)) not in rec["records"]:
                rec["records"].append(int(mm.group(3)))
            in_runlist = False
            continue
        if line.startswith("MFT Record Flags:"):
            rec["flags"] = line.split(":", 1)[1].split()
        elif line.startswith("Number of Hard Links:"):
            rec["link_count"] = num(line.split(":", 1)[1])
        if cur is None:
            continue
        s = line.strip()
        if s.startswith("Runlist:"):
            in_runlist = True
            cur["runlist"] = []
            continue
        if in_runlist:
            parts = s.split()
            if len(parts) == 3 and (parts[0].startswith("0x") or parts[0] == "0"):
                lcn = None if parts[1].startswith("<") else int(parts[1], 16)
                cur["runlist"].append([int(parts[0], 16), lcn, int(parts[2], 16)])
                continue
            in_runlist = False
        if ":" not in s:
            continue
        k, v = (x.strip() for x in s.split(":", 1))
        key = k.lower().replace(" ", "_")
        if k == "Attribute name":
            cur["name"] = v.strip("'")
        elif k == "Resident":
            cur["resident"] = v == "Yes"
        elif k == "Attribute flags":
            cur["flags"] = v
        elif k in ("Data size", "Allocated size", "Initialized size", "Compressed size") \
                and key not in cur and "list_references" not in cur:
            cur[key] = num(v)
        elif k == "Parent directory" and cur["type"] == "$FILE_NAME":
            cur["parent"] = num(v)
        elif k == "Namespace" and cur["type"] == "$FILE_NAME":
            cur["namespace"] = v
        elif k == "Filename" and cur["type"] == "$FILE_NAME":
            cur["filename"] = v[1:-1]
        elif k in ("Reparse tag", "Reparse point tag"):
            cur["reparse_tag"] = v.split()[0]
        elif k == "MFT reference":
            cur.setdefault("list_references", []).append(num(v))
    for a in rec["attributes"]:
        if a["type"] == "$FILE_NAME":
            rec["names"].append({k: a.get(k) for k in ("parent", "namespace", "filename")})
    return rec

# Children of directories the mount could not enter, from ntfsls.
for rel in m["unreachable"]:
    listing = subprocess.run([env["NTFSLS"], "-a", "-i", "-p", "/" + rel, img],
                             capture_output=True, text=True).stdout
    for line in listing.splitlines():
        parts = line.split(None, 1)
        if len(parts) != 2 or not parts[0].isdigit() or parts[1] in (".", ".."):
            continue
        inode, name = int(parts[0]), parts[1]
        info = ntfsinfo(inode)
        data = next((a for a in info["attributes"] if a["type"] == "$DATA" and "name" not in a), None)
        is_dir = "DIRECTORY" in info.get("flags", [])
        alloc = 0
        if data and not data.get("resident"):
            alloc = data.get("compressed_size", data.get("allocated_size", 0))
        elif data:
            alloc = (((data["data_size"] + 7) & ~7) + 511) // 512 * 512
        entries[rel + "/" + name] = {
            "inode": inode,
            "kind": "dir" if is_dir else "file",
            "size": data["data_size"] if data else 0,
            "allocated": alloc,
            "links": info.get("link_count"),
            "source": "ntfsls+ntfsinfo, because the mount cannot enter the parent; allocated follows ntfs-3g's rule",
        }

details = {}
for rel, e in sorted(entries.items()):
    info = ntfsinfo(e["inode"])
    e["parents"] = sorted({n["parent"] for n in info["names"] if n["parent"] is not None})
    named = {a["name"]: a["data_size"] for a in info["attributes"]
             if a["type"] == "$DATA" and "name" in a}
    if "streams" in e or named:
        # ntfs-3g lists no streams for reparse points it shows as symlinks.
        if not e.get("reparse_tag"):
            assert sorted(e.get("streams", [])) == sorted(named), (rel, e.get("streams"), named)
        e["streams"] = dict(sorted(named.items()))
    if e["inode"] in details:
        continue
    if rel.startswith("bigdir/") and rel not in ("bigdir/f00000.txt", "bigdir/f02999.txt"):
        continue
    details[e["inode"]] = info

attr_list_records = sorted(i for i, d in details.items()
                           if any(a["type"] == "$ATTRIBUTE_LIST" for a in d["attributes"]))
extensions = {str(i): sorted(r for r in details[i]["records"] if r != i) for i in attr_list_records}

# System files 0-15, for the parser's bootstrap ($MFT runlist and friends).
for i in range(16):
    info = ntfsinfo(i)
    if info["attributes"]:
        details.setdefault(i, info)

deleted = run(env["NTFSINFO"], "-i", str(deleted_inode), img)
assert all(e["inode"] != deleted_inode for e in entries.values())

with open(img, "rb") as f:
    raw = f.read()
boot = raw[:512]
bytes_per_sector, sectors_per_cluster = struct.unpack_from("<HB", boot, 11)
clusters_per_record = struct.unpack_from("<b", boot, 0x40)[0]
cluster = bytes_per_sector * sectors_per_cluster
record = cluster * clusters_per_record if clusters_per_record > 0 else 1 << -clusters_per_record
version = lambda exe: run(env[exe], "--version").strip().splitlines()[0]
mft = ntfsinfo(0)
mft_data = next(a for a in mft["attributes"] if a["type"] == "$DATA" and "name" not in a)

def raw_record(n):
    """Reads MFT record n straight from the image, fixups applied (a sanity check)."""
    pos = n * record
    for vcn, lcn, length in mft_data["runlist"]:
        if vcn * cluster <= pos < (vcn + length) * cluster:
            off = lcn * cluster + pos - vcn * cluster
            rec = bytearray(raw[off:off + record])
            usa_off, usa_count = struct.unpack_from("<HH", rec, 4)
            for i in range(1, usa_count):
                rec[i * bytes_per_sector - 2:i * bytes_per_sector] = rec[usa_off + 2 * i:usa_off + 2 * i + 2]
            return bytes(rec)
    raise ValueError(n)

del_rec = raw_record(deleted_inode)
assert del_rec[:4] == b"FILE"
del_flags = struct.unpack_from("<H", del_rec, 0x16)[0]
assert not del_flags & 1, "deleted record still in use"

manifest = {
    "image": "small.ntfs.gz",
    "image_bytes": len(raw),
    "sha256": hashlib.sha256(raw).hexdigest(),
    "cluster_size": cluster,
    "sector_size": bytes_per_sector,
    "mft_record_size": record,
    "mft_lcn": struct.unpack_from("<Q", boot, 0x30)[0],
    "mft_records": mft_data["data_size"] // record,
    "volume_serial": "0x%016X" % struct.unpack_from("<Q", boot, 0x48)[0],
    "volume_label": "vacancy-small",
    "tools": {"ntfs-3g": version("NTFS3G"), "mkntfs": version("MKNTFS"),
              "built": "ntfs-3g_ntfsprogs-2022.10.3 from source, --with-fuse=external (libfuse 2.9.9)"},
    "notes": [
        "inode is the MFT record number (ntfs-3g st_ino).",
        "size, allocated and links are what ntfs-3g reports: size is st_size, allocated is st_blocks*512, links is st_nlink.",
        "ntfs-3g allocated: non-resident data -> allocated size, or compressed size for compressed or sparse data; resident data -> value length rounded up to 8, which st_blocks turns into a multiple of 512; directories -> $INDEX_ALLOCATION allocated size (0 with only $INDEX_ROOT).",
        "ntfs-3g reports st_nlink 1 for directories, and shows reparse points as symlinks (posix_type l) whose size is the length of the link text it makes (or of its error text for tags it has no plugin for), not the NTFS data size; records.<inode>.attributes has the NTFS sizes.",
        "attributes is system.ntfs_attrib: the file attributes as ntfs-3g reports them.",
        "streams maps alternate data stream names (listed by ntfs-3g with streams_interface=xattr) to their data sizes (from ntfsinfo); the unnamed stream is not listed.",
        "records holds parsed ntfsinfo -v output for system records 0-15 and every record under the root, except bigdir files other than the first and last. runlist rows are [vcn, lcn or null for a hole, length] in clusters. Attribute flags 0x0001 compressed, 0x8000 sparse.",
        "ntfs-3g names files in the POSIX namespace; only the DOS-named folder has Win32 + DOS names.",
        "deleted.bin was written and then unlinked; its record is not in use (record_flags, read straight from the image).",
        "parents lists the parent directory record numbers from the record's $FILE_NAME attributes.",
    ],
    "attribute_list_records": attr_list_records,
    "extension_records": extensions,
    "dos_names": {rel: e["dos_name"] for rel, e in entries.items() if "dos_name" in e and rel},
    "deleted": {"name": "deleted.bin", "inode": deleted_inode, "size": 20000,
                "record_flags": "0x%04X" % del_flags,
                "name_still_in_record": "deleted.bin".encode("utf-16-le") in del_rec,
                "ntfsinfo": deleted.strip().splitlines()[:6]},
    "entries": dict(sorted(entries.items())),
    "records": {str(k): v for k, v in sorted(details.items())},
}
with open(out, "w", encoding="utf-8", newline="\n") as f:
    # One line per entry and per record keeps the file diffable and small.
    f.write("{\n")
    items = list(manifest.items())
    for n, (k, v) in enumerate(items):
        sep = ",\n" if n < len(items) - 1 else "\n"
        if k in ("entries", "records"):
            rows = [f"  {json.dumps(rk, ensure_ascii=False)}: {json.dumps(rv, ensure_ascii=False)}"
                    for rk, rv in v.items()]
            f.write(f" {json.dumps(k)}: {{\n" + ",\n".join(rows) + "\n }" + sep)
        else:
            f.write(f" {json.dumps(k)}: {json.dumps(v, ensure_ascii=False)}" + sep)
    f.write("}\n")
PY

# The image gzipped (16 MiB of mostly zeros and 0xFF log pages: about 280 KB), and what a listing of it shows,
# from the manifest (kept beside the image in $WORK; it is too big for the repository).
python3 -I -c 'import gzip,sys; open(sys.argv[2],"wb").write(gzip.compress(open(sys.argv[1],"rb").read(),9,mtime=0))' "$IMG" "$OUT/small.ntfs.gz"
python3 -I "$OUT/expect.py" "$WORK/small.manifest.json" "$OUT/small.expected.tsv"
echo "wrote $OUT/small.ntfs.gz and $OUT/small.expected.tsv (manifest: $WORK/small.manifest.json)"

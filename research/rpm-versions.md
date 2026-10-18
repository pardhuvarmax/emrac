# RPM version comparison (rpmvercmp)

Subject: `librpmio.so.10.7.0` from rpm-tools 6.0.1-2 (Arch), analysed in a throwaway container
copy. Ground truth from `rpm.vercmp()` through `rpm --eval '%{lua: ...}'`. Behavior only; no
decompiled code reproduced.

Note: `rpm.vercmp` compares two bare strings with the segment algorithm. It does **not** split
epoch/version/release. EVR splitting happens above it (`rpmdsCompare`/header code, not traced
here). So `1.0` vs `1.0-1` compares as two plain strings and gives -1.

## Algorithm (per call, on two strings)

Identical strings: 0. Otherwise loop:

1. Skip separators on both sides: anything that is not a letter, digit, `~` or `^`.
2. Tilde: if either side is at `~`, a tilde side is **older**. Two tildes cancel and the
   loop moves on one char each.
3. Caret: if either side is at `^`:
   - the other side is at end of string: the caret side is **newer** (`1.0^git` > `1.0`)
   - otherwise the caret side is **older** than anything that continues (`1.0^git` < `1.0.1`)
   - two carets cancel.
4. If one side ended: the side with remaining content is newer.
5. Take a maximal run from each side of the kind of the **first** side's char: digits, or
   letters. If the other side has a different kind there, the digit side is newer.
6. Digits: strip leading zeros, longer run is larger, else bytewise. Letters: bytewise.
7. Equal runs: continue.

If everything matches and one side has trailing content left, it is newer.

## Contrasts that matter

| case | pacman | dpkg | rpm |
|---|---|---|---|
| `1.0~rc1` vs `1.0` | newer | older | older |
| `1.0^git1` vs `1.0` | newer | newer | newer |
| `1.0^git1` vs `1.0.1` | n/a | newer | older |
| `1.0+1` vs `1.0.1` | equal | older | equal |
| `1.0_1` vs `1.0.1` | equal | newer | equal |
| `1.0.` vs `1.0` | newer | newer | equal |
| `a` vs `1` | older | newer | older |
| `1.0a` vs `1.0` | older | newer | newer |

pacman's routine is a descendant of rpm's but predates its `~` and `^` support; keep the
three implementations separate.

## Test cases

`research/version-truth-table.txt`, rpm column. Reproduce exactly; the `^` rows are the
likeliest to regress.

## Database layout (rpm 6.0.1, default sqlite backend)

Observed in a throwaway root (`rpm --root ... --initdb`, then one real package installed).
`rpm --showrc` reports `_db_backend sqlite`, `_dbpath %{_var}/lib/rpm`.

### Files

`<root>/var/lib/rpm/`: `rpmdb.sqlite` (the database), `rpmdb.sqlite-wal` and `-shm` (SQLite
write-ahead log, present while open and gone after a clean close), and `.rpm.lock` (empty
file locked for the duration of a transaction).

### Schema

- `Packages(hnum INTEGER PRIMARY KEY AUTOINCREMENT, blob BLOB NOT NULL)`: one row per
  installed package. `hnum` is the package's database id; it is never reused.
- One index table per searchable tag, all the same shape:
  `<Tag>(key, hnum, idx)` where `key` is the tag value, `hnum` points at `Packages`, and
  `idx` is the position of the value within the header array (e.g. which of a package's
  file names it is). Tables seen: `Name`, `Basenames`, `Dirnames`, `Group`, `Requirename`,
  `Providename`, `Conflictname`, `Obsoletename`, `Triggername`, `Filetriggername`,
  `Transfiletriggername`, `Recommendname`, `Suggestname`, `Supplementname`, `Enhancename`,
  `Installtid`, `Sigmd5`, `Sha1header` (the last three have BLOB or TEXT keys).
  `Name`, `Providename`, `Requirename`, `Basenames`, `Dirnames` hold one row per value.
- `Version` and `Release` are **not** indexed: they live only inside the header blob.
- `sqlite_stat1` and `sqlite_stat4` are query-planner statistics.

For the test package (`Epoch 1`, `Version 2.0~rc1`, `Release 3`, one `%config(noreplace)`
file and one plain file): `Name` = `rtest`; `Providename` = `config(rtest)` and `rtest`;
`Requirename` = `bash` and `config(rtest)`; `Basenames` = `rtest.conf`, `h.txt`;
`Dirnames` = `/etc/`, `/usr/share/rtest/` (directory names end with `/`; file names are
stored as basename plus directory index, not as full paths).

### The header blob

`Packages.blob` is the package header in rpm's binary header format, minus the 8-byte
magic: two big-endian 32-bit numbers, first the number of index entries, then the byte size
of the data area; then that many 16-byte index entries (tag, type, offset, count; all
big-endian); then the data area. Confirmed on the test row: 67 entries, 1604 data bytes,
`8 + 67*16 + 1604 = 2684` = the blob length. Version, release, epoch, file lists, sizes,
digests, scripts and dependency details are all inside it; `rpm -q --qf` decodes them
(`%{EPOCH}:%{VERSION}-%{RELEASE}` printed `1:2.0~rc1-3`).

### How rpm writes

- Lock: `.rpm.lock` opened read/write and locked; the sqlite file, `-wal` and `-shm` are
  opened read/write with `O_NOFOLLOW`.
- Each payload file is created under a temporary name in its final directory,
  `<name>;<8 hex digits>`, `O_CREAT|O_EXCL`, mode 0200, written, then `renameat`d over the
  final name. So rpm is atomic per file where pacman is not.

## Status

Algorithm: traced and confirmed. Database layout and file-write protocol: confirmed by
execution on one package. Not covered: decoding individual tag numbers beyond the ones
`rpm -q --qf` exposes, the rpm 4.x Berkeley-DB and ndb backends (not built here), upgrade
and erase ordering, `%pre/%post` script environment, triggers, and failure injection.

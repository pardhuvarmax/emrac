# Config file preview: what becomes `.pacnew` or `.pacsave`

The plan is [`../plan/r3/u5-pacnew-preview.md`](../plan/r3/u5-pacnew-preview.md); the code is
`crates/emrac-core/src/config_preview.rs` and `Sources::config_preview`. `emrac pacnew` handles these
files **after** an upgrade; this shows them **before**, under the plan:

```
Config files you changed that this plan touches:
  tp  /etc/tp.conf  the new version will be written as <file>.pacnew; yours stays (you edited it and the new package changes it too)
  nginx  /etc/nginx/nginx.conf  may get a <file>.pacnew; yours stays (you edited it; the new package is not downloaded yet, so whether it changes the file is not known)
  `emrac pacnew` helps with them afterwards.
```

## The rule

pacman compares three md5 sums of a `backup=()` file: **original** (recorded when the installed version was
installed), **current** (on disk) and **new** (in the new package). Every row below was produced by running
pacman 7.1 in a container (`research/pacman-commit-order.md`), not read from its source.

| situation | result |
|---|---|
| current = original (untouched) | replaced, no `.pacnew`, no message |
| edited, new = original (upstream unchanged) | yours kept, nothing beside it |
| edited, new = current (package already ships your version) | nothing |
| edited, new differs from both | yours kept, new version as `.pacnew` |
| file deleted by you | new file simply written |
| `NoUpgrade` lists it and it exists | always `.pacnew`, even if untouched and identical |
| `NoUpgrade` lists it and it is missing | written |
| new package no longer ships it, edited | kept as `.pacsave` |
| new package no longer ships it, untouched | removed |
| removal, edited | kept as `.pacsave` |
| removal, untouched or missing | nothing left |

## Certain and maybe

The **new** md5 exists only inside the new package. If that package is in the cache (a `pacman -Sw` ran, or
an earlier attempt), emrac extracts just that file and the answer is certain. Otherwise an edited file is
listed as *may get a `.pacnew`* and the reason says the package is not downloaded; emrac never downloads
to plan. "No longer ships it" is known from the repositories' file lists (`pacman -Fy`); without them it is
not reported. A file emrac cannot read (root only) is a maybe too.

Not part of the plan or its id (`PlanPreview`). `purge` keeps its own list of what it discards.

## Checked against pacman

`dev/e2e/preview.sh` builds a local repository with real `backup=` files and compares this list with the
`.pacnew` and `.pacsave` files pacman then leaves: before and after the download, an unchanged file, a
dropped file, a removal and `NoUpgrade`. 12 checks.

# Slice u23 — let pacman ask its own questions; no consent from silence

(Written after the fact: the problem was found while answering "can emrac pass
pacman's prompts through?")

## Problems

1. Every mutating command ran pacman with `--noconfirm`, so pacman could never
   ask. The decisions it asks about — a package conflict ("Remove jack2?"), a
   provider choice, a replacement, a key import — all took pacman's default,
   and the default for a conflict is *no*. A conflicting install could only
   fail, even when the user wanted it.
2. `confirm()` is documented to return false when it cannot read an answer, but
   end of input is not a read error: it came back as an empty line, which
   `[Y/n]` treats as yes. With a closed stdin and no `--yes`, `emrac install`
   proceeded **without anyone answering**.
3. Reading answers through `std::io::stdin()` takes a whole buffered block, so
   with piped input the first emrac prompt consumed every line and pacman, run
   afterwards, saw end of input.

## Design

- `--yes` keeps its meaning (fully non-interactive: `--noconfirm`). Without it,
  pacman runs with the terminal attached and **no** `--noconfirm`, so it asks.
  Applies to install, remove, purge, upgrade, `pacman -U` (rollback, downgrade,
  reinstall, restore, AUR installs) and `makepkg -s`'s dependency step.
  The sandboxed build steps stay `--noconfirm` (no prompt can occur there).
  Cost: pacman's own "Proceed with installation?" follows emrac's.
- Prompts read standard input one byte at a time up to the newline, so exactly
  one line is taken and the rest stays for pacman.
- **No answer is never consent**: end of input (or a read error) is a no,
  whatever the default. An empty *line* still takes the default.

## Not done (found on the way)

emrac's *plan* for an install does not list packages that conflict resolution
will remove (`install pipewire-jack` planned "1 package" and said nothing about
removing `jack2`); pacman's prompt now makes that visible at execution, but the
preview should say it. That is the same class of fault as u22 and is next.

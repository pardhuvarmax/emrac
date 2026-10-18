#!/usr/bin/env bash
# Runs inside the test container as the ordinary user (dev/e2e/run.sh bundle).
# Offline bundles: create from the repositories, verify, damage, apply; and the
# case a hash cannot catch: a bundle whose manifest was changed to match a bad
# signature, which pacman itself must refuse.
#
# last full run: after u14, 26 of 26 passed
e=/usr/local/bin/emrac
n=0; bad=0
ck() { n=$((n+1)); if eval "$2" >/tmp/ck.out 2>&1; then echo "PASS  $1"; else bad=$((bad+1)); echo "FAIL  $1"; head -8 /tmp/ck.out | sed 's/^/        /'; fi; }
sudo pacman -Syu --noconfirm >/dev/null 2>&1
sudo pacman -R --noconfirm sl cowsay >/dev/null 2>&1
W=/tmp/bw; rm -rf $W; mkdir -p $W; cd $W

ck "the help explains the old-keyring problem" "$e bundle apply --help | grep -q 'archlinux-keyring' && $e bundle apply --help | grep -q 'invalid or corrupted package' && $e bundle --help | grep -q 'OLD KEYRINGS'"
ck "a bundle of the keyring alone can be made" "$e bundle create $W/bk archlinux-keyring >$W/ck.out 2>&1; ls $W/bk/pkg | grep -q '^archlinux-keyring-' && ls $W/bk/pkg | grep -q 'archlinux-keyring-.*\\.sig\$'"
ck "create makes a bundle of a package and its dependencies" "$e bundle create $W/b1 sl >$W/c1.out 2>&1; grep -q 'packages' $W/c1.out && ls $W/b1/pkg | grep -q '^sl-.*pkg.tar'"
ck "every package has a signature file" "for f in \$(ls $W/b1/pkg | grep -v '\\.sig\$'); do [ -f $W/b1/pkg/\$f.sig ] || { echo no sig \$f; exit 1; }; done"
ck "the user can copy and delete the bundle" "cp -r $W/b1 $W/b1copy && rm -rf $W/b1copy"
ck "the manifest lists a checksum for each file" "grep -q 'sha256' $W/b1/manifest.toml && grep -q '^format = 1' $W/b1/manifest.toml"
ck "create will not use a folder that exists" "! $e bundle create $W/b1 sl 2>&1 && $e bundle create $W/b1 sl 2>&1 | grep -q 'already exists'"
ck "create with no packages and no --for is refused" "$e bundle create $W/b0 2>&1 | grep -q 'name the packages'"
ck "a package the repositories do not have is refused, and no folder is left" "$e bundle create $W/b9 definitely-not-a-package 2>&1 | grep -q 'no repository provides' && [ ! -e $W/b9 ]"
ck "verify passes on a good bundle" "$e bundle verify $W/b1 2>&1 | grep -q 'bundle is complete'"

# damage
cp -r $W/b1 $W/b2
F=$(ls $W/b2/pkg | grep -v '\.sig$' | head -1)
printf 'X' | dd of=$W/b2/pkg/$F bs=1 seek=100 conv=notrunc 2>/dev/null
ck "a changed byte is found by verify" "! $e bundle verify $W/b2 2>&1 && $e bundle verify $W/b2 2>&1 | grep -q 'does not match its checksum'"
ck "apply refuses a damaged bundle and installs nothing" "! $e bundle apply $W/b2 --yes 2>&1 && ! pacman -Q sl"
cp -r $W/b1 $W/b3; rm $W/b3/pkg/$(ls $W/b3/pkg | grep '\.sig$' | head -1)
ck "a missing signature file is found" "$e bundle verify $W/b3 2>&1 | grep -q 'signature file is missing'"
cp -r $W/b1 $W/b4; touch $W/b4/pkg/unlisted.pkg.tar.zst
ck "a file nobody listed is found" "$e bundle verify $W/b4 2>&1 | grep -q 'not in the manifest'"

# apply
ck "apply --dry-run shows the install and changes nothing" "$e bundle apply $W/b1 --dry-run 2>&1 | grep -q 'install   sl' && ! pacman -Q sl"
ck "apply installs through the normal path" "$e bundle apply $W/b1 --yes </dev/null >$W/a1.out 2>&1; pacman -Q sl && grep -q 'pre-flight' $W/a1.out"
ck "the install is in the history" "$e history 2>&1 | grep -q 'sl'"
ck "applying it again finds nothing to do" "$e bundle apply $W/b1 --yes </dev/null 2>&1 | grep -q 'nothing to install'"

# --for: only what the target lacks
sudo pacman -R --noconfirm sl >/dev/null 2>&1
$e export --packages -o $W/target.toml >/dev/null 2>&1
ck "--for leaves out what the target already has" "$e bundle create $W/b5 sl cowsay --for $W/target.toml >$W/c5.out 2>&1; ls $W/b5/pkg | grep -q '^sl-' && ls $W/b5/pkg | grep -q '^cowsay-' && ! ls $W/b5/pkg | grep -q '^glibc-'"
ck "--for with nothing to do says so" "$e bundle create $W/b6 pacman --for $W/target.toml 2>&1 | grep -q 'nothing to bundle'"

# an upgrade bundle: pretend the target has an older sl
sudo pacman -S --noconfirm sl >/dev/null 2>&1
$e export --packages -o $W/old.toml >/dev/null 2>&1
python_free_edit() { awk 'BEGIN{r=0} /^name = "sl"$/ {r=1} r==1 && /^version = / {print "version = \"1.0-1\""; r=0; next} {print}' $W/old.toml > $W/old2.toml; }
python_free_edit
ck "with --for and no names, the target's pending upgrades are bundled" "$e bundle create $W/b7 --for $W/old2.toml >/dev/null 2>&1; ls $W/b7/pkg | grep -q '^sl-'"

# a bad signature that the manifest was altered to agree with
sudo pacman -R --noconfirm sl >/dev/null 2>&1
cp -r $W/b1 $W/b8
SIG=$(ls $W/b8/pkg | grep '^sl-.*\.sig$' | head -1)
head -c 200 /dev/urandom > $W/b8/pkg/$SIG
NEW=$(sha256sum $W/b8/pkg/$SIG | cut -d' ' -f1)
OLD=$(grep -A8 'name = "sl"' $W/b8/manifest.toml | grep sig_sha256 | head -1 | sed 's/.*"\(.*\)"/\1/')
sed -i "s/$OLD/$NEW/" $W/b8/manifest.toml
ck "verify cannot tell: the manifest was made to agree" "$e bundle verify $W/b8 2>&1 | grep -q 'bundle is complete'"
ck "pacman itself refuses the bad signature and nothing is installed" "! $e bundle apply $W/b8 --yes </dev/null 2>&1; ! pacman -Q sl"

# never a downgrade
sudo pacman -S --noconfirm sl >/dev/null 2>&1
cp -r $W/b1 $W/b10
V=$(pacman -Q sl | cut -d' ' -f2)
sed -i "s/sl-$V-/sl-0.1-1-/g; s/version = \"$V\"/version = \"0.1-1\"/" $W/b10/manifest.toml
for f in $W/b10/pkg/sl-$V-*; do mv "$f" "$(echo $f | sed "s/sl-$V-/sl-0.1-1-/")"; done
ck "a bundle older than what is installed is refused, not applied" "$e bundle apply $W/b10 --dry-run 2>&1 | grep -q 'never downgraded' && [ \"\$(pacman -Q sl | cut -d' ' -f2)\" = \"$V\" ]"
ck "--json gives the changes" "$e bundle apply $W/b1 --dry-run --json | grep -q '\"kind\"'"
ck "a folder with no manifest is refused" "mkdir -p $W/empty && ! $e bundle verify $W/empty 2>&1"

echo; echo "$((n-bad)) of $n passed"
[ "$bad" -eq 0 ]

#!/usr/bin/env bash
# Tests install.sh in the disposable test container (see dev/container): the
# options, the refusals (not Arch, no terminal, wrong checksum), a real install
# piped through sh the way `curl | sh` runs it, a second run, and the removal.
# dev/e2e/run.sh install mounts the script at /install.sh and runs this.
# Needs network access; the real install takes about a minute.
sudo pacman -Sy --noconfirm >/dev/null 2>&1
cp /install.sh /tmp/install.sh
echo "### 1. --help"; sh /tmp/install.sh --help | head -3
echo "### 2. unknown option fails"; sh /tmp/install.sh --bogus >/dev/null 2>&1; echo "exit $?"
echo "### 3. not an Arch system is refused (fake os-release)"
printf 'ID=ubuntu\n' > /tmp/os-release; sed 's|/etc/os-release|/tmp/os-release|g' /tmp/install.sh > /tmp/install-ubuntu.sh; sh /tmp/install-ubuntu.sh --dry-run 2>&1 | head -2
echo "### 4. --dry-run shows the plan and installs nothing"; sh /tmp/install.sh --dry-run 2>&1 | grep -E "will be installed|sudo pacman -U|dry run"; pacman -Q emrac 2>&1 | head -1
echo "### 5. no terminal and no --yes: it refuses to guess"; sh /tmp/install.sh </dev/null 2>&1 | tail -1; pacman -Q emrac 2>&1 | head -1
echo "### 6. a wrong pinned checksum stops before building"; sed 's/^SHA256=.*/SHA256="0000000000000000000000000000000000000000000000000000000000000000"/' /tmp/install.sh > /tmp/install-badsum.sh; sh /tmp/install-badsum.sh --yes 2>&1 | grep -E "does not match|expected|got |nothing was built" | head -4; pacman -Q emrac 2>&1 | head -1
echo "### 7. the real install, piped like curl | sh"; (time (cat /tmp/install.sh | sh -s -- --yes) ) > /tmp/install.log 2>&1; echo "exit $?"; grep -E "checksum matches|Finished making|Installing emrac|emrac 0.2.0|built by|done\." /tmp/install.log | head -8
pacman -Q emrac; ls -1 /tmp | grep -c emrac-install; command -v emrac; emrac --version | head -1
echo "### 8. a second run recognises it"; sh /tmp/install.sh --yes 2>&1 | tail -1
echo "### 9. --uninstall --yes"; sh /tmp/install.sh --uninstall --yes 2>&1 | tail -3; pacman -Q emrac 2>&1 | head -1
